// Copyright © The Daybrite Project
// SPDX-License-Identifier: MPL-2.0
//! Native DNS-SD discovery and advertisement. No async runtime is required.
//!
//! Operations own their native registrations. Dropping an operation cancels it;
//! `stop` waits for the coordinator to request native cleanup. Mobile OS teardown
//! may complete asynchronously. Call `next_event().await` from any executor, or use
//! `recv_timeout` on a worker thread. Events contain protocol data, not UI copy.
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fmt,
    future::Future,
    net::SocketAddr,
    pin::Pin,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    task::{Context, Poll, Waker},
    thread,
    time::Duration,
};

pub const ENUMERATION_TYPE: &str = "_services._dns-sd._udp";
const MAX_SERVICES: usize = 1024;
const MAX_TYPES: usize = 128;

#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    Unsupported,
    InvalidInput(String),
    PermissionDenied,
    Unavailable(String),
    Native { code: i64, message: String },
    Overflow,
    Cancelled,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}
pub type Result<T, E = Error> = std::result::Result<T, E>;

pub(crate) fn trace_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var("DAY_ZEROCONF_TRACE").is_ok_and(|v| v == "1"))
}
// Called by workers, never from a native callback. Ignore stderr write failures.
pub(crate) fn trace(message: impl fmt::Display) {
    if trace_enabled() {
        use std::io::Write;
        let _ = writeln!(std::io::stderr().lock(), "zeroconf: {message}");
    }
}

/// A service observation belongs to an interface. Names are never persistent device IDs.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ServiceId {
    pub name: String,
    pub service_type: String,
    pub domain: String,
    pub interface: u32,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Service {
    pub id: ServiceId,
    pub hostname: String,
    pub port: u16,
    pub addresses: Vec<SocketAddr>,
    pub txt: TxtRecord,
}
/// Values are opaque bytes. None (key only) differs from Some(empty) (key=).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TxtRecord(pub Vec<(String, Option<Vec<u8>>)>);
impl TxtRecord {
    pub fn get(&self, key: &str) -> Option<&[u8]> {
        self.0
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .and_then(|(_, v)| v.as_deref())
    }
    pub fn encode(&self) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        let mut seen = BTreeSet::new();
        for (key, value) in &self.0 {
            if key.is_empty()
                || !key.bytes().all(|b| (0x20..=0x7e).contains(&b) && b != b'=')
                || !seen.insert(key.to_ascii_lowercase())
            {
                return Err(Error::InvalidInput("TXT key".into()));
            }
            let n = key.len() + value.as_ref().map_or(0, |v| v.len() + 1);
            if n > 255 {
                return Err(Error::InvalidInput("TXT item exceeds 255 bytes".into()));
            }
            out.push(n as u8);
            out.extend_from_slice(key.as_bytes());
            if let Some(v) = value {
                out.push(b'=');
                out.extend_from_slice(v)
            }
        }
        if out.len() > 1300 {
            return Err(Error::InvalidInput(
                "TXT record exceeds portable 1300 byte limit".into(),
            ));
        }
        Ok(out)
    }
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let mut rest = bytes;
        let mut result = Self::default();
        let mut seen = BTreeSet::new();
        while let Some((&n, tail)) = rest.split_first() {
            let n = n as usize;
            if tail.len() < n {
                return Err(Error::InvalidInput("truncated TXT record".into()));
            }
            let item = &tail[..n];
            rest = &tail[n..];
            if item.is_empty() {
                continue;
            }
            let split = item.iter().position(|b| *b == b'=');
            let key = &item[..split.unwrap_or(n)];
            let Ok(key) = std::str::from_utf8(key) else {
                continue;
            };
            if key.is_empty()
                || !key.bytes().all(|b| (0x20..=0x7e).contains(&b))
                || !seen.insert(key.to_ascii_lowercase())
            {
                continue;
            }
            result
                .0
                .push((key.into(), split.map(|i| item[i + 1..].to_vec())));
        }
        Ok(result)
    }
}

#[derive(Clone, Debug)]
pub struct BrowseOptions {
    /// None enumerates advertised service types, then browses each.
    pub service_type: Option<String>,
    pub interface: u32,
}
impl BrowseOptions {
    pub fn all() -> Self {
        Self {
            service_type: None,
            interface: 0,
        }
    }
    pub fn service(service_type: impl Into<String>) -> Self {
        Self {
            service_type: Some(service_type.into()),
            interface: 0,
        }
    }
}
#[derive(Clone, Debug)]
pub struct Advertisement {
    pub name: String,
    pub service_type: String,
    pub port: u16,
    pub txt: TxtRecord,
    pub interface: u32,
}
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Event {
    Started,
    ServiceType(String),
    Found(ServiceId),
    Resolved(Service),
    Removed(ServiceId),
    Registered(ServiceId),
    Error(Error),
    Stopped,
}
#[derive(Clone, Copy, Debug)]
pub struct Capabilities {
    /// A backend exists; availability and permissions are checked when it starts.
    pub native: bool,
    /// Enumerates service types. Mobile fallback uses the default IPv4 multicast route.
    pub browse_all: bool,
    /// Arbitrary byte values can be advertised without conversion.
    pub binary_txt: bool,
    /// Key-only TXT attributes can be advertised without becoming empty values.
    pub key_only_txt: bool,
    /// A caller may choose a native interface index instead of zero (automatic).
    pub interface_selection: bool,
}
pub fn capabilities() -> Capabilities {
    backend::capabilities()
}

pub fn validate_type(value: &str) -> Result<String> {
    let value = value.trim_end_matches('.');
    let value = value.strip_suffix(".local").unwrap_or(value);
    let Some((name, transport)) = value.split_once('.') else {
        return Err(Error::InvalidInput("service type".into()));
    };
    if !name.starts_with('_')
        || name.len() < 2
        || name.len() > 63
        || !name[1..]
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        || !matches!(transport, "_tcp" | "_udp")
    {
        return Err(Error::InvalidInput("service type".into()));
    }
    Ok(value.to_owned())
}

struct Queue {
    events: VecDeque<Event>,
    closed: bool,
    waker: Option<Waker>,
}
struct Inbox {
    state: Mutex<Queue>,
    ready: Condvar,
}
impl Inbox {
    fn push(&self, event: Event) {
        let mut s = self.state.lock().unwrap();
        if s.closed {
            return;
        }
        if s.events.len() >= MAX_SERVICES {
            s.events.clear();
            s.events.push_back(Event::Error(Error::Overflow));
            s.closed = true
        } else {
            s.events.push_back(event)
        }
        let waker = s.waker.take();
        drop(s);
        if let Some(w) = waker {
            w.wake();
        }
        self.ready.notify_all();
    }
    fn close(&self) {
        let mut s = self.state.lock().unwrap();
        s.closed = true;
        let waker = s.waker.take();
        drop(s);
        if let Some(w) = waker {
            w.wake();
        }
        self.ready.notify_all()
    }
}
/// A single-consumer event subscription. Native callbacks never run application code.
pub struct Operation {
    inbox: Arc<Inbox>,
    cancelled: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}
/// Cancels an operation without blocking the UI thread. Clones share one cancellation flag.
#[derive(Clone)]
pub struct Cancellation(Arc<AtomicBool>);
impl Cancellation {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }
}
impl Operation {
    pub fn cancellation(&self) -> Cancellation {
        Cancellation(self.cancelled.clone())
    }
    pub fn next_event(&mut self) -> Next<'_> {
        Next(self)
    }
    pub fn recv_timeout(&mut self, timeout: Duration) -> Option<Event> {
        let s = self.inbox.state.lock().unwrap();
        let (mut s, _) = self
            .inbox
            .ready
            .wait_timeout_while(s, timeout, |s| s.events.is_empty() && !s.closed)
            .unwrap();
        s.events.pop_front()
    }
    pub fn stop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
        if let Some(t) = self.worker.take() {
            let _ = t.join();
        }
        self.inbox.close()
    }
}
impl Drop for Operation {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
        self.inbox.close();
    }
}
pub struct Next<'a>(&'a mut Operation);
impl Future for Next<'_> {
    type Output = Option<Event>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut s = self.0.inbox.state.lock().unwrap();
        if let Some(e) = s.events.pop_front() {
            Poll::Ready(Some(e))
        } else if s.closed {
            Poll::Ready(None)
        } else {
            s.waker = Some(cx.waker().clone());
            Poll::Pending
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) enum Request {
    Browse(String, u32),
    Resolve(ServiceId),
    Advertise(Advertisement),
}
#[derive(Clone)]
pub(crate) struct Sink {
    sender: mpsc::SyncSender<(u64, Event)>,
    generation: u64,
    overflow: Arc<AtomicBool>,
}
pub(crate) trait Native: Send {}
impl<T: Send> Native for T {}
pub(crate) fn emit(sink: &Sink, event: Event) {
    if let Err(mpsc::TrySendError::Full(_)) = sink.sender.try_send((sink.generation, event)) {
        sink.overflow.store(true, Ordering::Release);
    }
}

/// Browse local services, automatically resolving each observation. Keep the operation alive.
pub fn browse(mut options: BrowseOptions) -> Result<Operation> {
    if let Some(t) = &options.service_type {
        options.service_type = Some(validate_type(t)?)
    }
    start(Request::Browse(
        options
            .service_type
            .unwrap_or_else(|| ENUMERATION_TYPE.into()),
        options.interface,
    ))
}
/// Resolve a known local identity. Some backends emit updates; refresh to guarantee freshness.
pub fn resolve(mut id: ServiceId) -> Result<Operation> {
    id.service_type = validate_type(&id.service_type)?;
    if id.name.is_empty()
        || id.name.len() > 63
        || id.name.contains('\0')
        || !matches!(id.domain.as_str(), "local" | "local.")
    {
        return Err(Error::InvalidInput("local service identity".into()));
    }
    id.domain = "local.".into();
    start(Request::Resolve(id))
}
/// Advertise a listener owned by the caller. Registered reports the actual instance name.
/// Dropping the returned operation withdraws the advertisement; it does not close the listener.
pub fn advertise(mut service: Advertisement) -> Result<Operation> {
    service.service_type = validate_type(&service.service_type)?;
    if service.name.is_empty()
        || service.name.len() > 63
        || service.name.contains('\0')
        || service.port == 0
    {
        return Err(Error::InvalidInput("service name or port".into()));
    }
    service.txt.encode()?;
    start(Request::Advertise(service))
}
fn start(request: Request) -> Result<Operation> {
    if !capabilities().native {
        return Err(Error::Unsupported);
    }
    let inbox = Arc::new(Inbox {
        state: Mutex::new(Queue {
            events: VecDeque::new(),
            closed: false,
            waker: None,
        }),
        ready: Condvar::new(),
    });
    let cancelled = Arc::new(AtomicBool::new(false));
    let output = inbox.clone();
    let stop = cancelled.clone();
    let worker = thread::Builder::new()
        .name("zeroconf".into())
        .spawn(move || {
            match &request {
                Request::Advertise(a) => trace(format_args!(
                    "advertise {:?} {} port={} interface={}",
                    a.name, a.service_type, a.port, a.interface
                )),
                _ => trace(format_args!("starting {request:?}")),
            }
            let (sender, rx) = mpsc::sync_channel(MAX_SERVICES);
            let overflow = Arc::new(AtomicBool::new(false));
            let tx = Sink {
                sender,
                generation: 0,
                overflow: overflow.clone(),
            };
            let browsing = matches!(&request, Request::Browse(..));
            let all = matches!(&request,Request::Browse(t,_) if t==ENUMERATION_TYPE);
            let interface = match &request {
                Request::Browse(_, i) => *i,
                Request::Resolve(id) => id.interface,
                Request::Advertise(a) => a.interface,
            };
            let mut handles = Vec::new();
            let mut types = BTreeSet::new();
            let mut resolves: BTreeMap<ServiceId, (Box<dyn Native>, u64)> = BTreeMap::new();
            let mut next_generation = 1;
            emit(&tx, Event::Started);
            match backend::start(request, tx.clone()) {
                Ok(h) => handles.push(h),
                Err(e) => {
                    output.push(Event::Error(e));
                    output.close();
                    return;
                }
            }
            while !stop.load(Ordering::Acquire) {
                if overflow.load(Ordering::Acquire) {
                    output.push(Event::Error(Error::Overflow));
                    break;
                }
                if output.state.lock().unwrap().closed {
                    break;
                }
                let (generation, event) = match rx.recv_timeout(Duration::from_millis(50)) {
                    Ok(e) => e,
                    Err(mpsc::RecvTimeoutError::Timeout) => continue,
                    Err(_) => break,
                };
                match &event {
                    Event::Resolved(s) => trace(format_args!(
                        "generation {generation}: resolved {:?}, port={}, address_count={}, txt_count={}",
                        s.id, s.port, s.addresses.len(), s.txt.0.len()
                    )),
                    _ => trace(format_args!("generation {generation}: {event:?}")),
                }
                // Ignore callbacks queued by a resolver that has already been removed.
                if browsing && generation != 0 && !resolves.values().any(|(_, g)| *g == generation)
                {
                    continue;
                }
                match &event {
                    Event::Resolved(service) if browsing && !resolves.contains_key(&service.id) => {
                        continue;
                    }
                    Event::ServiceType(t) if all => {
                        if validate_type(t).is_err() {
                            continue;
                        }
                        if !types.insert(t.clone()) {
                            continue;
                        }
                        if types.len() > MAX_TYPES {
                            output.push(Event::Error(Error::Overflow));
                            break;
                        }
                        match backend::start(Request::Browse(t.clone(), interface), tx.clone()) {
                            Ok(h) => handles.push(h),
                            Err(e) => output.push(Event::Error(e)),
                        }
                    }
                    Event::Found(id) if !resolves.contains_key(id) => {
                        if resolves.len() >= MAX_SERVICES {
                            output.push(Event::Error(Error::Overflow));
                            break;
                        }
                        let mut resolution_sink = tx.clone();
                        resolution_sink.generation = next_generation;
                        next_generation += 1;
                        let generation = resolution_sink.generation;
                        match backend::start(Request::Resolve(id.clone()), resolution_sink) {
                            Ok(h) => {
                                resolves.insert(id.clone(), (h, generation));
                            }
                            Err(e) => output.push(Event::Error(e)),
                        }
                    }
                    Event::Removed(id) => {
                        resolves.remove(id);
                    }
                    _ => {}
                }
                output.push(event);
            }
            drop(resolves);
            drop(handles);
            output.push(Event::Stopped);
            output.close();
        })
        .map_err(|e| Error::Unavailable(e.to_string()))?;
    Ok(Operation {
        inbox,
        cancelled,
        worker: Some(worker),
    })
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
#[path = "apple.rs"]
mod backend;
#[cfg(all(target_os = "linux", not(target_env = "ohos")))]
#[path = "linux.rs"]
mod backend;
#[cfg(target_os = "windows")]
#[path = "windows.rs"]
mod backend;
#[cfg(any(target_os = "android", target_env = "ohos"))]
#[path = "mobile.rs"]
mod backend;
#[cfg(any(test, target_os = "windows"))]
mod windows_names;
#[cfg(not(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "android",
    target_os = "linux",
    target_os = "windows"
)))]
mod backend {
    use super::*;
    pub fn capabilities() -> Capabilities {
        Capabilities {
            native: false,
            browse_all: false,
            binary_txt: false,
            key_only_txt: false,
            interface_selection: false,
        }
    }
    pub fn start(_: Request, _: Sink) -> Result<Box<dyn Native>> {
        Err(Error::Unsupported)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn txt_binary_and_empty_roundtrip() {
        let t = TxtRecord(vec![
            ("flag".into(), None),
            ("empty".into(), Some(vec![])),
            ("binary".into(), Some(vec![0, 255, b'=', 1])),
        ]);
        assert_eq!(TxtRecord::decode(&t.encode().unwrap()).unwrap(), t);
    }
    #[test]
    fn txt_invalid_and_duplicate() {
        assert!(TxtRecord::decode(&[4, 1]).is_err());
        assert!(
            TxtRecord(vec![("A".into(), None), ("a".into(), None)])
                .encode()
                .is_err()
        );
        assert_eq!(
            TxtRecord::decode(&[3, b'a', b'=', b'1', 3, b'A', b'=', b'2'])
                .unwrap()
                .get("a"),
            Some(&b"1"[..])
        );
    }
    #[test]
    fn types_are_validated() {
        assert_eq!(
            validate_type("_calibre._tcp.local.").unwrap(),
            "_calibre._tcp"
        );
        for s in ["http", "_http._tcp.evil", "_http._sctp", "_x\0._tcp"] {
            assert!(validate_type(s).is_err());
        }
        assert!(validate_type(&format!("_{}._tcp", "a".repeat(62))).is_ok());
        assert!(validate_type(&format!("_{}._tcp", "a".repeat(63))).is_err());
    }
    #[test]
    fn oversized_txt_rejected() {
        assert!(
            TxtRecord(vec![("x".into(), Some(vec![0; 255]))])
                .encode()
                .is_err()
        );
    }
}

#[cfg(any(test, target_os = "android", target_env = "ohos"))]
mod wire;
#[cfg(test)]
mod lifecycle_tests {
    use super::*;
    fn inbox() -> Arc<Inbox> {
        Arc::new(Inbox {
            state: Mutex::new(Queue {
                events: VecDeque::new(),
                closed: false,
                waker: None,
            }),
            ready: Condvar::new(),
        })
    }
    #[test]
    fn slow_consumer_is_told_to_resync() {
        let q = inbox();
        for _ in 0..=MAX_SERVICES {
            q.push(Event::Started)
        }
        let s = q.state.lock().unwrap();
        assert!(s.closed);
        assert_eq!(s.events.len(), 1);
        assert_eq!(s.events.front(), Some(&Event::Error(Error::Overflow)));
    }
    #[test]
    fn full_native_channel_signals_overflow_without_blocking() {
        let (sender, _) = mpsc::sync_channel(1);
        let overflow = Arc::new(AtomicBool::new(false));
        let sink = Sink {
            sender,
            generation: 0,
            overflow: overflow.clone(),
        };
        emit(&sink, Event::Started);
        emit(&sink, Event::Stopped); /* Disconnected is harmless after cancellation. */
        assert!(!overflow.load(Ordering::Acquire));
        let (sender, _rx) = mpsc::sync_channel(1);
        let sink = Sink {
            sender,
            generation: 0,
            overflow: overflow.clone(),
        };
        emit(&sink, Event::Started);
        emit(&sink, Event::Stopped);
        assert!(overflow.load(Ordering::Acquire));
    }
    #[test]
    fn dropping_operation_closes_reader_and_cancels() {
        let q = inbox();
        let c = Arc::new(AtomicBool::new(false));
        let op = Operation {
            inbox: q.clone(),
            cancelled: c.clone(),
            worker: None,
        };
        drop(op);
        assert!(c.load(Ordering::Acquire));
        assert!(q.state.lock().unwrap().closed);
    }
    #[test]
    fn invalid_identity_rejected_before_native_call() {
        let id = ServiceId {
            name: "bad\0name".into(),
            service_type: "_http._tcp".into(),
            domain: "local.".into(),
            interface: 0,
        };
        assert!(resolve(id).is_err());
    }
}

/// Whether this host requires an explicit runtime permission request before discovery.
/// Currently Android 17+ apps targeting API 37+. Request ACCESS_LOCAL_NETWORK through
/// day-part-permissions; Apple presents its own prompt on first native access.
pub fn requires_explicit_permission() -> bool {
    #[cfg(any(target_os = "android", target_env = "ohos"))]
    {
        backend::requires_explicit_permission()
    }
    #[cfg(not(any(target_os = "android", target_env = "ohos")))]
    {
        false
    }
}

#[cfg(any(test, target_os = "android", target_env = "ohos"))]
mod enumerate;
