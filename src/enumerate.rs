// Copyright © The Daybrite Project
// SPDX-License-Identifier: MPL-2.0
//! Narrow legacy-unicast DNS-SD type query for hosts whose native browser rejects
//! _services._dns-sd._udp. This is not a responder, resolver, or mDNS cache.
//! RFC 6762 §6.7: queries from an ephemeral port receive unicast replies.
//! Also set QU for Android responders that route answers by QU rather than source port.
//! Accept their standard mDNS ID zero as well as the echoed legacy query ID.
use super::*;
use std::{io, net::UdpSocket, time::Instant};
fn io_error(e: io::Error) -> Error {
    if e.kind() == io::ErrorKind::PermissionDenied {
        Error::PermissionDenied
    } else {
        Error::Unavailable(e.to_string())
    }
}
const NAME: &str = "_services._dns-sd._udp.local";
fn query(id: u16) -> Vec<u8> {
    let mut b = vec![];
    b.extend(id.to_be_bytes());
    b.extend([0, 0, 0, 1, 0, 0, 0, 0, 0, 0]);
    for label in NAME.split('.') {
        b.push(label.len() as u8);
        b.extend(label.as_bytes())
    }
    b.extend([0, 0, 12, 0x80, 1]);
    b
}
fn number(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(b.get(at..at + 2)?.try_into().ok()?))
}
/// Follow compression pointers with a hard hop/expanded-name limit, including forward pointers.
fn name(b: &[u8], cursor: &mut usize) -> Option<String> {
    let mut at = *cursor;
    let mut jumped = false;
    let mut labels = vec![];
    let mut size = 0;
    for _ in 0..128 {
        let n = *b.get(at)?;
        if n & 0xc0 == 0xc0 {
            let low = *b.get(at + 1)?;
            if !jumped {
                *cursor = at + 2;
                jumped = true
            }
            at = usize::from(n & 0x3f) * 256 + usize::from(low);
            continue;
        }
        if n & 0xc0 != 0 {
            return None;
        }
        at += 1;
        if n == 0 {
            if !jumped {
                *cursor = at
            }
            return Some(labels.join("."));
        }
        let n = n as usize;
        size += n + 1;
        if size > 255 {
            return None;
        }
        let label = String::from_utf8_lossy(b.get(at..at + n)?);
        labels.push(
            label
                .to_ascii_lowercase()
                .replace('\\', "\\092")
                .replace('.', "\\046"),
        );
        at += n;
    }
    None
}
fn types(b: &[u8], id: u16) -> Option<Vec<String>> {
    if b.len() < 12
        || (number(b, 0)? != 0 && number(b, 0)? != id)
        || number(b, 2)? & 0x800f != 0x8000
    {
        return None;
    }
    let questions = number(b, 4)?;
    let count = u32::from(number(b, 6)?) + u32::from(number(b, 8)?) + u32::from(number(b, 10)?);
    if questions > 64 || count > 1024 {
        return None;
    }
    let mut at = 12;
    for _ in 0..questions {
        name(b, &mut at)?;
        at = at.checked_add(4)?;
        if at > b.len() {
            return None;
        }
    }
    let mut out = vec![];
    for _ in 0..count {
        let owner = name(b, &mut at)?;
        let kind = number(b, at)?;
        let class = number(b, at + 2)? & 0x7fff;
        let ttl = u32::from_be_bytes(b.get(at + 4..at + 8)?.try_into().ok()?);
        let length = number(b, at + 8)? as usize;
        at += 10;
        let end = at.checked_add(length)?;
        if end > b.len() {
            return None;
        }
        if owner == NAME && kind == 12 && class == 1 && ttl > 0 {
            let mut cursor = at;
            let target = name(b, &mut cursor)?;
            if cursor > end {
                return None;
            }
            if let Ok(t) = validate_type(&target)
                && !out.contains(&t)
            {
                out.push(t)
            }
        }
        at = end;
    }
    Some(out)
}
struct Handle(Arc<AtomicBool>, Option<thread::JoinHandle<()>>);
impl Drop for Handle {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
        if let Some(t) = self.1.take() {
            let _ = t.join();
        }
    }
}
pub(crate) fn start(sink: Sink) -> Result<Box<dyn Native>> {
    let socket = UdpSocket::bind(("0.0.0.0", 0)).map_err(io_error)?;
    start_query(socket, "224.0.0.251:5353".parse().unwrap(), sink)
}
fn start_query(
    socket: UdpSocket,
    destination: std::net::SocketAddr,
    sink: Sink,
) -> Result<Box<dyn Native>> {
    socket
        .set_read_timeout(Some(Duration::from_millis(200)))
        .map_err(io_error)?;
    socket.set_multicast_ttl_v4(255).map_err(io_error)?;
    let id = socket.local_addr().map_err(io_error)?.port();
    let packet = query(id);
    let stop = Arc::new(AtomicBool::new(false));
    let cancelled = stop.clone();
    let worker = thread::Builder::new()
        .name("dns-sd-types".into())
        .spawn(move || {
            let mut next = Instant::now();
            let mut seen = BTreeSet::new();
            let mut bytes = [0; 9000];
            while !cancelled.load(Ordering::Acquire) {
                if Instant::now() >= next {
                    if let Err(e) = socket.send_to(&packet, destination) {
                        emit(&sink, Event::Error(io_error(e)));
                        break;
                    }
                    next = Instant::now() + Duration::from_secs(5);
                }
                match socket.recv_from(&mut bytes) {
                    Ok((n, source)) => {
                        if source.port() != destination.port() {
                            continue;
                        }
                        if let Some(types) = types(&bytes[..n], id) {
                            for t in types {
                                if seen.insert(t.clone()) {
                                    if seen.len() > MAX_TYPES {
                                        emit(&sink, Event::Error(Error::Overflow));
                                        return;
                                    }
                                    emit(&sink, Event::ServiceType(t))
                                }
                            }
                        }
                    }
                    Err(e)
                        if matches!(
                            e.kind(),
                            io::ErrorKind::WouldBlock
                                | io::ErrorKind::TimedOut
                                | io::ErrorKind::Interrupted
                        ) => {}
                    Err(e) => {
                        emit(&sink, Event::Error(io_error(e)));
                        break;
                    }
                }
            }
        })
        .map_err(io_error)?;
    Ok(Box::new(Handle(stop, Some(worker))))
}
#[cfg(test)]
mod tests {
    use super::*;
    fn response() -> Vec<u8> {
        let mut b = query(123);
        b[2] = 0x84;
        b[7] = 1;
        b.extend([0xc0, 12, 0, 12, 0, 1, 0, 0, 0, 120, 0, 18]);
        b.extend([5]);
        b.extend(b"_http");
        b.push(4);
        b.extend(b"_tcp");
        b.push(5);
        b.extend(b"local");
        b.push(0);
        b
    }
    #[test]
    fn compression_and_ptr() {
        assert_eq!(types(&response(), 123), Some(vec!["_http._tcp".into()]));
        assert_eq!(types(&response(), 124), None)
    }
    #[test]
    fn pointer_cycles_and_truncation() {
        let b = response();
        for end in 0..b.len() {
            assert!(types(&b[..end], 123).is_none())
        }
        let mut b = response();
        let start = query(123).len();
        b[start] = 0xc0;
        b[start + 1] = start as u8;
        assert!(types(&b, 123).is_none())
    }
    #[test]
    fn ttl_zero_ignored() {
        let mut b = response();
        let start = query(123).len();
        b[start + 9] = 0;
        assert_eq!(types(&b, 123), Some(vec![]))
    }
    #[test]
    fn unicast_transport_filters_packets_deduplicates_and_cancels() {
        // An actual loopback UDP exchange. Hosted VMs need not route multicast back
        // to themselves for us to test the mobile query/receive/cancellation path.
        let responder = UdpSocket::bind("127.0.0.1:0").unwrap();
        responder
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let destination = responder.local_addr().unwrap();
        let (sender, rx) = mpsc::sync_channel(64);
        let handle = start_query(
            UdpSocket::bind("127.0.0.1:0").unwrap(),
            destination,
            Sink {
                sender,
                generation: 0,
                overflow: Arc::new(AtomicBool::new(false)),
            },
        )
        .unwrap();
        let mut bytes = [0; 1024];
        let (len, client) = responder.recv_from(&mut bytes).unwrap();
        assert_eq!(&bytes[len - 2..len], &[0x80, 1]); // QU + IN
        let mut answer = response();
        answer[..2].copy_from_slice(&bytes[..2]);
        responder
            .send_to(&answer[..answer.len() - 1], client)
            .unwrap(); // malformed
        responder.send_to(&answer, client).unwrap();
        // Android responders return ID zero; this is the same observation.
        answer[..2].fill(0);
        responder.send_to(&answer, client).unwrap();
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(2)).unwrap().1,
            Event::ServiceType("_http._tcp".into())
        );
        assert!(rx.recv_timeout(Duration::from_millis(300)).is_err());
        let before = Instant::now();
        drop(handle);
        assert!(before.elapsed() < Duration::from_secs(2));
    }
    #[test]
    #[ignore = "requires a native mDNS responder and local network access"]
    fn native_legacy_query() {
        let mut publisher = advertise(Advertisement {
            name: "Legacy query test fixture".into(),
            service_type: "_dayzc-query._tcp".into(),
            port: 12345,
            txt: TxtRecord::default(),
            interface: 0,
        })
        .unwrap();
        let until = Instant::now() + Duration::from_secs(30);
        loop {
            assert!(
                Instant::now() < until,
                "native responder registration timed out"
            );
            match publisher.recv_timeout(Duration::from_millis(100)) {
                Some(Event::Registered(_)) => break,
                Some(Event::Error(e)) => panic!("native responder registration: {e}"),
                _ => {}
            }
        }
        let (sender, rx) = mpsc::sync_channel(64);
        let _handle = start(Sink {
            sender,
            generation: 0,
            overflow: Arc::new(AtomicBool::new(false)),
        })
        .unwrap();
        loop {
            assert!(
                Instant::now() < until,
                "no multicast reply for _dayzc-query._tcp; check the runner's multicast route and local-network permission"
            );
            match rx.recv_timeout(Duration::from_millis(100)) {
                Ok((_, Event::ServiceType(t))) if t == "_dayzc-query._tcp" => break,
                Ok((_, Event::Error(e))) => panic!("multicast enumeration query: {e}"),
                _ => {}
            }
        }
        publisher.stop();
    }
}
