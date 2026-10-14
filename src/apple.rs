// Copyright © The Daybrite Project
// SPDX-License-Identifier: MPL-2.0
//! Bonjour C API. Each operation owns a serial poll thread and its callback storage.
use super::*;
use std::ffi::{CStr, CString, c_char, c_void};
use std::ptr;
type Ref = *mut c_void;
type BrowseFn = unsafe extern "C" fn(
    Ref,
    u32,
    u32,
    i32,
    *const c_char,
    *const c_char,
    *const c_char,
    *mut c_void,
);
type ResolveFn = unsafe extern "C" fn(
    Ref,
    u32,
    u32,
    i32,
    *const c_char,
    *const c_char,
    u16,
    u16,
    *const c_void,
    *mut c_void,
);
type RegisterFn =
    unsafe extern "C" fn(Ref, u32, i32, *const c_char, *const c_char, *const c_char, *mut c_void);
type AddressFn = unsafe extern "C" fn(
    Ref,
    u32,
    u32,
    i32,
    *const c_char,
    *const libc::sockaddr,
    u32,
    *mut c_void,
);
#[link(name = "System")]
unsafe extern "C" {
    fn DNSServiceBrowse(
        r: *mut Ref,
        flags: u32,
        interface: u32,
        kind: *const c_char,
        domain: *const c_char,
        cb: BrowseFn,
        context: *mut c_void,
    ) -> i32;
    fn DNSServiceResolve(
        r: *mut Ref,
        flags: u32,
        interface: u32,
        name: *const c_char,
        kind: *const c_char,
        domain: *const c_char,
        cb: ResolveFn,
        context: *mut c_void,
    ) -> i32;
    fn DNSServiceRegister(
        r: *mut Ref,
        flags: u32,
        interface: u32,
        name: *const c_char,
        kind: *const c_char,
        domain: *const c_char,
        host: *const c_char,
        port: u16,
        txt_len: u16,
        txt: *const c_void,
        cb: RegisterFn,
        context: *mut c_void,
    ) -> i32;
    fn DNSServiceGetAddrInfo(
        r: *mut Ref,
        flags: u32,
        interface: u32,
        protocol: u32,
        host: *const c_char,
        cb: AddressFn,
        context: *mut c_void,
    ) -> i32;
    fn DNSServiceRefSockFD(r: Ref) -> i32;
    fn DNSServiceProcessResult(r: Ref) -> i32;
    fn DNSServiceRefDeallocate(r: Ref);
}
fn error(code: i32) -> Error {
    if code == -65570 {
        Error::PermissionDenied
    } else {
        Error::Native {
            code: code.into(),
            message: "Bonjour".into(),
        }
    }
}
unsafe fn string(p: *const c_char) -> String {
    if p.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
    }
}
struct Context {
    sink: Sink,
    request: Request,
    service: Option<Service>,
    address_changed: bool,
}
unsafe extern "C" fn browse_cb(
    _: Ref,
    flags: u32,
    interface: u32,
    code: i32,
    name: *const c_char,
    kind: *const c_char,
    domain: *const c_char,
    ctx: *mut c_void,
) {
    let c = unsafe { &mut *ctx.cast::<Context>() };
    if code != 0 {
        emit(&c.sink, Event::Error(error(code)));
        return;
    }
    let (name, kind, domain) = unsafe { (string(name), string(kind), string(domain)) };
    if matches!(&c.request,Request::Browse(t,_) if t==ENUMERATION_TYPE) {
        if flags & 2 != 0
            && let Ok(t) = validate_type(&format!("{}.{}", name, kind.trim_end_matches('.')))
        {
            emit(&c.sink, Event::ServiceType(t));
        }
        return;
    }
    let id = ServiceId {
        name,
        service_type: kind.trim_end_matches('.').into(),
        domain,
        interface,
    };
    emit(
        &c.sink,
        if flags & 2 != 0 {
            Event::Found(id)
        } else {
            Event::Removed(id)
        },
    );
}
unsafe extern "C" fn resolve_cb(
    _: Ref,
    _: u32,
    interface: u32,
    code: i32,
    _: *const c_char,
    host: *const c_char,
    port: u16,
    len: u16,
    txt: *const c_void,
    ctx: *mut c_void,
) {
    let c = unsafe { &mut *ctx.cast::<Context>() };
    if code != 0 {
        emit(&c.sink, Event::Error(error(code)));
        return;
    }
    let Request::Resolve(id) = &c.request else {
        return;
    };
    let mut id = id.clone();
    id.interface = interface;
    let bytes = if len == 0 {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(txt.cast::<u8>(), len as usize) }
    };
    match TxtRecord::decode(bytes) {
        Ok(txt) => {
            c.service = Some(Service {
                id,
                hostname: unsafe { string(host) },
                port: u16::from_be(port),
                addresses: vec![],
                txt,
            });
            c.address_changed = true;
        }
        Err(e) => emit(&c.sink, Event::Error(e)),
    }
}
unsafe extern "C" fn address_cb(
    _: Ref,
    flags: u32,
    interface: u32,
    code: i32,
    _: *const c_char,
    address: *const libc::sockaddr,
    _: u32,
    ctx: *mut c_void,
) {
    let c = unsafe { &mut *ctx.cast::<Context>() };
    if code != 0 {
        emit(&c.sink, Event::Error(error(code)));
        return;
    }
    let Some(s) = &mut c.service else { return };
    if address.is_null() {
        return;
    }
    let addr = match unsafe { (*address).sa_family } as i32 {
        libc::AF_INET => {
            let a = unsafe { &*address.cast::<libc::sockaddr_in>() };
            SocketAddr::new(
                std::net::Ipv4Addr::from(a.sin_addr.s_addr.to_ne_bytes()).into(),
                s.port,
            )
        }
        libc::AF_INET6 => {
            let a = unsafe { &*address.cast::<libc::sockaddr_in6>() };
            let ip = std::net::Ipv6Addr::from(a.sin6_addr.s6_addr);
            let scope = if a.sin6_scope_id != 0 {
                a.sin6_scope_id
            } else if ip.is_unicast_link_local() {
                interface
            } else {
                0
            };
            SocketAddr::V6(std::net::SocketAddrV6::new(ip, s.port, 0, scope))
        }
        _ => return,
    };
    if flags & 2 != 0 {
        if !s.addresses.contains(&addr) {
            s.addresses.push(addr)
        }
    } else {
        s.addresses.retain(|a| *a != addr)
    }
    emit(&c.sink, Event::Resolved(s.clone()));
}
unsafe extern "C" fn register_cb(
    _: Ref,
    _: u32,
    code: i32,
    name: *const c_char,
    kind: *const c_char,
    domain: *const c_char,
    ctx: *mut c_void,
) {
    let c = unsafe { &*ctx.cast::<Context>() };
    if code != 0 {
        emit(&c.sink, Event::Error(error(code)));
        return;
    }
    let interface = if let Request::Advertise(a) = &c.request {
        a.interface
    } else {
        0
    };
    emit(
        &c.sink,
        Event::Registered(ServiceId {
            name: unsafe { string(name) },
            service_type: unsafe { string(kind) }.trim_end_matches('.').into(),
            domain: unsafe { string(domain) },
            interface,
        }),
    );
}
struct Handle {
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}
impl Drop for Handle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}
pub fn capabilities() -> Capabilities {
    Capabilities {
        native: true,
        browse_all: true,
        binary_txt: true,
        key_only_txt: true,
        interface_selection: true,
    }
}
fn cs(s: &str) -> Result<CString> {
    CString::new(s).map_err(|_| Error::InvalidInput("NUL in DNS name".into()))
}
pub fn start(request: Request, sink: Sink) -> Result<Box<dyn Native>> {
    // Validate before moving data into a native callback context.
    let (name, kind, domain) = match &request {
        Request::Browse(t, _) => (cs("")?, cs(t)?, cs("local.")?),
        Request::Resolve(id) => (cs(&id.name)?, cs(&id.service_type)?, cs(&id.domain)?),
        Request::Advertise(a) => (cs(&a.name)?, cs(&a.service_type)?, cs("local.")?),
    };
    let stop = Arc::new(AtomicBool::new(false));
    let cancelled = stop.clone();
    let t = thread::Builder::new()
        .name("bonjour".into())
        .spawn(move || {
            let mut ctx = Box::new(Context {
                sink,
                request,
                service: None,
                address_changed: false,
            });
            let ptr = (&mut *ctx as *mut Context).cast();
            let mut root = ptr::null_mut();
            let mut addresses = ptr::null_mut();
            let code = unsafe {
                match &ctx.request {
                    Request::Browse(_, i) => DNSServiceBrowse(
                        &mut root,
                        0,
                        *i,
                        kind.as_ptr(),
                        domain.as_ptr(),
                        browse_cb,
                        ptr,
                    ),
                    Request::Resolve(id) => DNSServiceResolve(
                        &mut root,
                        0,
                        id.interface,
                        name.as_ptr(),
                        kind.as_ptr(),
                        domain.as_ptr(),
                        resolve_cb,
                        ptr,
                    ),
                    Request::Advertise(a) => {
                        let txt = a.txt.encode().unwrap_or_default();
                        DNSServiceRegister(
                            &mut root,
                            0,
                            a.interface,
                            name.as_ptr(),
                            kind.as_ptr(),
                            domain.as_ptr(),
                            ptr::null(),
                            a.port.to_be(),
                            txt.len() as u16,
                            txt.as_ptr().cast(),
                            register_cb,
                            ptr,
                        )
                    }
                }
            };
            if code != 0 {
                emit(&ctx.sink, Event::Error(error(code)));
                return;
            }
            while !cancelled.load(Ordering::Acquire) {
                let refs = [root, addresses];
                let mut fds = refs.map(|r| libc::pollfd {
                    fd: if r.is_null() {
                        -1
                    } else {
                        unsafe { DNSServiceRefSockFD(r) }
                    },
                    events: libc::POLLIN,
                    revents: 0,
                });
                let status = unsafe { libc::poll(fds.as_mut_ptr(), 2, 100) };
                if status < 0 {
                    if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                        continue;
                    }
                    break;
                }
                if fds
                    .iter()
                    .any(|fd| fd.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0)
                {
                    emit(
                        &ctx.sink,
                        Event::Error(Error::Unavailable("Bonjour connection closed".into())),
                    );
                    break;
                }
                for (r, fd) in refs.into_iter().zip(fds) {
                    if fd.revents & libc::POLLIN != 0 {
                        let code = unsafe { DNSServiceProcessResult(r) };
                        if code != 0 {
                            emit(&ctx.sink, Event::Error(error(code)));
                            cancelled.store(true, Ordering::Release);
                        }
                    }
                }
                if ctx.address_changed {
                    ctx.address_changed = false;
                    if !addresses.is_null() {
                        unsafe { DNSServiceRefDeallocate(addresses) };
                        addresses = ptr::null_mut()
                    }
                    if let Some(s) = &ctx.service {
                        emit(&ctx.sink, Event::Resolved(s.clone()));
                        if let Ok(host) = cs(&s.hostname) {
                            let code = unsafe {
                                DNSServiceGetAddrInfo(
                                    &mut addresses,
                                    0,
                                    s.id.interface,
                                    0,
                                    host.as_ptr(),
                                    address_cb,
                                    ptr,
                                )
                            };
                            if code != 0 {
                                emit(&ctx.sink, Event::Error(error(code)));
                            }
                        }
                    }
                }
            }
            // All callbacks are synchronous in ProcessResult; close refs before freeing context.
            unsafe {
                if !addresses.is_null() {
                    DNSServiceRefDeallocate(addresses)
                }
                DNSServiceRefDeallocate(root)
            }
        })
        .map_err(|e| Error::Unavailable(e.to_string()))?;
    Ok(Box::new(Handle {
        stop,
        thread: Some(t),
    }))
}
