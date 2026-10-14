// Copyright © The Daybrite Project
// SPDX-License-Identifier: MPL-2.0
//! Windows 10+ DNS-SD, using dnsapi.dll (no Bonjour installation).
use super::*;
use std::{
    ffi::c_void,
    net::{Ipv4Addr, Ipv6Addr, SocketAddrV6},
    ptr,
    sync::atomic::AtomicU32,
};
use windows_sys::Win32::NetworkManagement::Dns::*;
const PENDING: u32 = 9506;
fn hostname() -> Result<Vec<u16>> {
    use windows_sys::Win32::System::SystemInformation::{
        ComputerNameDnsHostname, GetComputerNameExW,
    };
    let mut text = vec![0u16; 256];
    let mut len = text.len() as u32;
    // SAFETY: the native call receives a writable buffer and its length in UTF-16 units.
    if unsafe { GetComputerNameExW(ComputerNameDnsHostname, text.as_mut_ptr(), &mut len) } == 0 {
        return Err(Error::Unavailable("GetComputerNameExW".into()));
    }
    text.truncate(len as usize);
    text.extend(".local.".encode_utf16());
    text.push(0);
    Ok(text)
}
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
unsafe fn string(p: *const u16) -> String {
    if p.is_null() {
        return String::new();
    }
    let mut n = 0;
    unsafe {
        while n < 65536 && *p.add(n) != 0 {
            n += 1
        }
        String::from_utf16_lossy(std::slice::from_raw_parts(p, n))
    }
}
fn escape(s: &str) -> String {
    s.replace('\\', "\\092").replace('.', "\\046")
}
fn unescape(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' && i + 3 < b.len() && b[i + 1..i + 4].iter().all(u8::is_ascii_digit) {
            let n = (b[i + 1] - b'0') as u16 * 100
                + (b[i + 2] - b'0') as u16 * 10
                + (b[i + 3] - b'0') as u16;
            if n <= 255 {
                out.push(n as u8);
                i += 4;
                continue;
            }
        }
        out.push(b[i]);
        i += 1
    }
    String::from_utf8_lossy(&out).into_owned()
}
fn error(code: u32) -> Error {
    if code == 5 {
        Error::PermissionDenied
    } else {
        Error::Native {
            code: code.into(),
            message: "Windows DNS-SD".into(),
        }
    }
}
struct Context {
    sink: Sink,
    request: Request,
    done: AtomicBool,
    status: AtomicU32,
    registering: AtomicBool,
}
unsafe extern "system" fn browse_callback(
    status: u32,
    data: *const c_void,
    records: *const DNS_RECORDW,
) {
    unsafe {
        let c = &*data.cast::<Context>();
        if status == 1223 || status == 995 {
            if !records.is_null() {
                DnsFree(records.cast(), DnsFreeRecordList)
            }
            c.done.store(true, Ordering::Release);
            return;
        }
        if status != 0 {
            emit(&c.sink, Event::Error(error(status)));
        } else if let Request::Browse(kind, interface) = &c.request {
            let mut p = records;
            while let Some(r) = p.as_ref() {
                if r.wType == DNS_TYPE_PTR {
                    let target = string(r.Data.PTR.pNameHost);
                    let target = target.trim_end_matches('.');
                    if kind == ENUMERATION_TYPE {
                        if r.dwTtl > 0
                            && let Ok(t) = validate_type(target)
                        {
                            emit(&c.sink, Event::ServiceType(t));
                        }
                    } else {
                        let suffix = format!(".{kind}.local");
                        if let Some(name) = target.strip_suffix(&suffix) {
                            let id = ServiceId {
                                name: unescape(name),
                                service_type: kind.clone(),
                                domain: "local.".into(),
                                interface: *interface,
                            };
                            emit(
                                &c.sink,
                                if r.dwTtl == 0 {
                                    Event::Removed(id)
                                } else {
                                    Event::Found(id)
                                },
                            );
                        }
                    }
                }
                p = r.pNext;
            }
        }
        if !records.is_null() {
            DnsFree(records.cast(), DnsFreeRecordList)
        }
    }
}
unsafe fn service(c: &Context, s: &DNS_SERVICE_INSTANCE) -> Service {
    unsafe {
        let id = match &c.request {
            Request::Resolve(id) => id.clone(),
            Request::Advertise(a) => {
                let full = string(s.pszInstanceName);
                let suffix = format!(".{}.local", a.service_type);
                ServiceId {
                    name: unescape(
                        full.trim_end_matches('.')
                            .strip_suffix(&suffix)
                            .unwrap_or(&a.name),
                    ),
                    service_type: a.service_type.clone(),
                    domain: "local.".into(),
                    interface: a.interface,
                }
            }
            _ => unreachable!(),
        };
        let mut addresses = vec![];
        if let Some(ip) = s.ip4Address.as_ref() {
            addresses.push(SocketAddr::new(
                Ipv4Addr::from(ip.to_ne_bytes()).into(),
                s.wPort,
            ))
        }
        if let Some(ip) = s.ip6Address.as_ref() {
            let ip = Ipv6Addr::from(ip.IP6Byte);
            let scope = if ip.is_unicast_link_local() {
                s.dwInterfaceIndex
            } else {
                0
            };
            addresses.push(SocketAddr::V6(SocketAddrV6::new(ip, s.wPort, 0, scope)))
        }
        let mut txt = TxtRecord::default();
        if !s.keys.is_null() && !s.values.is_null() {
            for i in 0..s.dwPropertyCount.min(1024) as usize {
                let key = string(*s.keys.add(i));
                let p = *s.values.add(i);
                txt.0.push((
                    key,
                    if p.is_null() {
                        None
                    } else {
                        Some(string(p).into_bytes())
                    },
                ));
            }
        }
        Service {
            id,
            hostname: string(s.pszHostName),
            port: s.wPort,
            addresses,
            txt,
        }
    }
}
unsafe extern "system" fn resolve_callback(
    status: u32,
    data: *const c_void,
    instance: *const DNS_SERVICE_INSTANCE,
) {
    unsafe {
        let c = &*data.cast::<Context>();
        if status == 0 {
            if let Some(s) = instance.as_ref() {
                emit(&c.sink, Event::Resolved(service(c, s)));
            }
        } else if status != 1223 && status != 995 {
            emit(&c.sink, Event::Error(error(status)));
        }
        if !instance.is_null() {
            DnsServiceFreeInstance(instance)
        }
        c.status.store(status, Ordering::Release);
        c.done.store(true, Ordering::Release);
    }
}
unsafe extern "system" fn register_callback(
    status: u32,
    data: *const c_void,
    instance: *const DNS_SERVICE_INSTANCE,
) {
    unsafe {
        let c = &*data.cast::<Context>();
        if status == 0 && c.registering.load(Ordering::Acquire) {
            if let Some(s) = instance.as_ref() {
                emit(&c.sink, Event::Registered(service(c, s).id));
            }
        } else if status != 0 && status != 1223 && status != 995 {
            emit(&c.sink, Event::Error(error(status)));
        }
        if !instance.is_null() {
            DnsServiceFreeInstance(instance)
        }
        c.status.store(status, Ordering::Release);
        c.done.store(true, Ordering::Release);
    }
}
struct Handle {
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}
impl Drop for Handle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(w) = self.worker.take() {
            let _ = w.join();
        }
    }
}
pub fn capabilities() -> Capabilities {
    Capabilities {
        native: true,
        browse_all: true,
        binary_txt: false,
        key_only_txt: false,
        interface_selection: true,
    }
}
pub fn start(request: Request, sink: Sink) -> Result<Box<dyn Native>> {
    if let Request::Advertise(a) = &request {
        for (_, v) in &a.txt.0 {
            if v.as_ref()
                .is_none_or(|v| v.contains(&0) || std::str::from_utf8(v).is_err())
            {
                return Err(Error::Unsupported);
            }
        }
    }
    let stop = Arc::new(AtomicBool::new(false));
    let stopping = stop.clone();
    let worker = thread::Builder::new()
        .name("dns-sd".into())
        .spawn(move || unsafe {
            // Context, request, query, cancel handle and instance stay alive through the final callback.
            // In particular, DnsServiceBrowseCancel is asynchronous; its return is not a lifetime fence.
            let mut c = Box::new(Context {
                sink,
                request,
                done: AtomicBool::new(false),
                status: AtomicU32::new(PENDING),
                registering: AtomicBool::new(true),
            });
            let data = (&mut *c as *mut Context).cast();
            let mut cancel = DNS_SERVICE_CANCEL::default();
            match &c.request {
                Request::Browse(kind, interface) => {
                    let query = wide(&format!("{kind}.local."));
                    let req = DNS_SERVICE_BROWSE_REQUEST {
                        Version: 1,
                        InterfaceIndex: *interface,
                        QueryName: query.as_ptr(),
                        Anonymous: DNS_SERVICE_BROWSE_REQUEST_0 {
                            pBrowseCallback: Some(browse_callback),
                        },
                        pQueryContext: data,
                    };
                    let code = DnsServiceBrowse(&req, &mut cancel) as u32;
                    if code != PENDING {
                        emit(&c.sink, Event::Error(error(code)));
                        return;
                    }
                    while !stopping.load(Ordering::Acquire) {
                        thread::sleep(Duration::from_millis(30));
                    }
                    let code = DnsServiceBrowseCancel(&cancel);
                    if code != 0 {
                        emit(&c.sink, Event::Error(error(code as u32)));
                    }
                    while !c.done.load(Ordering::Acquire) {
                        thread::sleep(Duration::from_millis(10));
                    }
                }
                Request::Resolve(id) => {
                    let mut query = wide(&format!(
                        "{}.{}.{}",
                        escape(&id.name),
                        id.service_type,
                        id.domain
                    ));
                    let req = DNS_SERVICE_RESOLVE_REQUEST {
                        Version: 1,
                        InterfaceIndex: id.interface,
                        QueryName: query.as_mut_ptr(),
                        pResolveCompletionCallback: Some(resolve_callback),
                        pQueryContext: data,
                    };
                    let code = DnsServiceResolve(&req, &mut cancel) as u32;
                    if code != PENDING {
                        emit(&c.sink, Event::Error(error(code)));
                        return;
                    }
                    let mut cancelling = false;
                    while !c.done.load(Ordering::Acquire) {
                        if stopping.load(Ordering::Acquire) && !cancelling {
                            DnsServiceResolveCancel(&cancel);
                            cancelling = true;
                        }
                        thread::sleep(Duration::from_millis(10));
                    }
                }
                Request::Advertise(a) => {
                    let name = wide(&format!("{}.{}.local.", escape(&a.name), a.service_type));
                    let host = match hostname() {
                        Ok(host) => host,
                        Err(e) => {
                            emit(&c.sink, Event::Error(e));
                            return;
                        }
                    };
                    let keys: Vec<_> = a.txt.0.iter().map(|(k, _)| wide(k)).collect();
                    let values: Vec<_> = a
                        .txt
                        .0
                        .iter()
                        .map(|(_, v)| v.as_ref().map(|v| wide(std::str::from_utf8(v).unwrap())))
                        .collect();
                    let kp: Vec<_> = keys.iter().map(|k| k.as_ptr()).collect();
                    let vp: Vec<_> = values
                        .iter()
                        .map(|v| v.as_ref().map_or(ptr::null(), |v| v.as_ptr()))
                        .collect();
                    let instance = DnsServiceConstructInstance(
                        name.as_ptr(),
                        host.as_ptr(),
                        ptr::null(),
                        ptr::null(),
                        a.port,
                        0,
                        0,
                        kp.len() as u32,
                        kp.as_ptr(),
                        vp.as_ptr(),
                    );
                    if instance.is_null() {
                        emit(
                            &c.sink,
                            Event::Error(Error::Unavailable("DnsServiceConstructInstance".into())),
                        );
                        return;
                    }
                    let req = DNS_SERVICE_REGISTER_REQUEST {
                        Version: 1,
                        InterfaceIndex: a.interface,
                        pServiceInstance: instance,
                        pRegisterCompletionCallback: Some(register_callback),
                        pQueryContext: data,
                        ..Default::default()
                    };
                    let code = DnsServiceRegister(&req, &mut cancel);
                    if code != PENDING {
                        DnsServiceFreeInstance(instance);
                        emit(&c.sink, Event::Error(error(code)));
                        return;
                    }
                    let mut cancelling = false;
                    while !c.done.load(Ordering::Acquire) {
                        if stopping.load(Ordering::Acquire) && !cancelling {
                            DnsServiceRegisterCancel(&cancel);
                            cancelling = true;
                        }
                        thread::sleep(Duration::from_millis(10));
                    }
                    if c.status.load(Ordering::Acquire) == 0 {
                        while !stopping.load(Ordering::Acquire) {
                            thread::sleep(Duration::from_millis(30));
                        }
                        c.registering.store(false, Ordering::Release);
                        c.done.store(false, Ordering::Release);
                        let code = DnsServiceDeRegister(&req, &mut cancel);
                        if code == PENDING {
                            while !c.done.load(Ordering::Acquire) {
                                thread::sleep(Duration::from_millis(10));
                            }
                        } else if code != 0 {
                            emit(&c.sink, Event::Error(error(code)));
                        }
                    }
                    DnsServiceFreeInstance(instance);
                }
            }
        })
        .map_err(|e| Error::Unavailable(e.to_string()))?;
    Ok(Box::new(Handle {
        stop,
        worker: Some(worker),
    }))
}
