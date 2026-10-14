// Copyright © The Daybrite Project
// SPDX-License-Identifier: MPL-2.0
//! Avahi's native client and simple-poll API, loaded only when an operation starts.
use super::*;
use std::ffi::{CStr, CString, c_char, c_void};
use std::ptr;
type P = *mut c_void;
type ClientCb = unsafe extern "C" fn(P, i32, P);
type BrowseCb =
    unsafe extern "C" fn(P, i32, i32, i32, *const c_char, *const c_char, *const c_char, u32, P);
type TypeCb = unsafe extern "C" fn(P, i32, i32, i32, *const c_char, *const c_char, u32, P);
type ResolveCb = unsafe extern "C" fn(
    P,
    i32,
    i32,
    i32,
    *const c_char,
    *const c_char,
    *const c_char,
    *const c_char,
    P,
    u16,
    P,
    u32,
    P,
);
macro_rules! api {($($name:ident: $ty:ty),* $(,)?)=>{
 struct Api { _client:libloading::Library,_common:libloading::Library,$($name:$ty),* }
 impl Api {fn load()->Result<Self>{unsafe{
  let client=libloading::Library::new("libavahi-client.so.3").map_err(|e|Error::Unavailable(e.to_string()))?;
  let common=libloading::Library::new("libavahi-common.so.3").map_err(|e|Error::Unavailable(e.to_string()))?;
  $(let $name= *client.get::<$ty>(concat!(stringify!($name),"\0").as_bytes()).or_else(|_|common.get::<$ty>(concat!(stringify!($name),"\0").as_bytes())).map_err(|e|Error::Unavailable(e.to_string()))?;)*
  Ok(Self{_client:client,_common:common,$($name),*})
 }}}
}}
api! {
 avahi_simple_poll_new:unsafe extern "C" fn()->P,
 avahi_simple_poll_get:unsafe extern "C" fn(P)->P,
 avahi_simple_poll_iterate:unsafe extern "C" fn(P,i32)->i32,
 avahi_simple_poll_free:unsafe extern "C" fn(P),
 avahi_client_new:unsafe extern "C" fn(P,i32,ClientCb,P,*mut i32)->P,
 avahi_client_free:unsafe extern "C" fn(P),
 avahi_client_get_state:unsafe extern "C" fn(P)->i32,
 avahi_client_errno:unsafe extern "C" fn(P)->i32,
 avahi_service_browser_new:unsafe extern "C" fn(P,i32,i32,*const c_char,*const c_char,u32,BrowseCb,P)->P,
 avahi_service_type_browser_new:unsafe extern "C" fn(P,i32,i32,*const c_char,u32,TypeCb,P)->P,
 avahi_service_resolver_new:unsafe extern "C" fn(P,i32,i32,*const c_char,*const c_char,*const c_char,i32,u32,ResolveCb,P)->P,
 avahi_entry_group_new:unsafe extern "C" fn(P,ClientCb,P)->P,
 avahi_entry_group_add_service_strlst:unsafe extern "C" fn(P,i32,i32,u32,*const c_char,*const c_char,*const c_char,*const c_char,u16,P)->i32,
 avahi_entry_group_commit:unsafe extern "C" fn(P)->i32,
 avahi_string_list_add_arbitrary:unsafe extern "C" fn(P,*const u8,usize)->P,
 avahi_string_list_free:unsafe extern "C" fn(P),
 avahi_string_list_serialize:unsafe extern "C" fn(P,*mut u8,usize)->usize,
 avahi_address_snprint:unsafe extern "C" fn(*mut c_char,usize,P)->*mut c_char,
}
struct Context {
    api: Arc<Api>,
    sink: Sink,
    request: Request,
    seen: std::cell::RefCell<BTreeMap<ServiceId, BTreeSet<i32>>>,
    addresses: std::cell::RefCell<Vec<SocketAddr>>,
}
unsafe fn s(p: *const c_char) -> String {
    if p.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
    }
}
fn failure(code: i32) -> Error {
    if code == -20 {
        Error::PermissionDenied
    } else {
        Error::Native {
            code: code.into(),
            message: "Avahi".into(),
        }
    }
}
unsafe extern "C" fn client_cb(client: P, state: i32, data: P) {
    if state == 100 {
        let c = unsafe { &*data.cast::<Context>() };
        emit(
            &c.sink,
            Event::Error(failure(unsafe { (c.api.avahi_client_errno)(client) })),
        )
    }
}
unsafe extern "C" fn types_cb(
    _: P,
    _: i32,
    _: i32,
    event: i32,
    kind: *const c_char,
    _: *const c_char,
    _: u32,
    data: P,
) {
    let c = unsafe { &*data.cast::<Context>() };
    if event == 0 {
        emit(&c.sink, Event::ServiceType(unsafe { s(kind) }))
    } else if event == 2 {
        emit(&c.sink, Event::Error(failure(-1)))
    }
}
unsafe extern "C" fn browse_cb(
    _: P,
    interface: i32,
    protocol: i32,
    event: i32,
    name: *const c_char,
    kind: *const c_char,
    domain: *const c_char,
    _: u32,
    data: P,
) {
    let c = unsafe { &*data.cast::<Context>() };
    if event == 2 {
        emit(&c.sink, Event::Error(failure(-1)));
        return;
    }
    if event > 1 {
        return;
    }
    let id = ServiceId {
        name: unsafe { s(name) },
        service_type: unsafe { s(kind) },
        domain: format!("{}.", unsafe { s(domain) }.trim_end_matches('.')),
        interface: interface.max(0) as u32,
    };
    let mut seen = c.seen.borrow_mut();
    if event == 0 {
        let set = seen.entry(id.clone()).or_default();
        let first = set.is_empty();
        set.insert(protocol);
        if first {
            emit(&c.sink, Event::Found(id))
        }
    } else if let Some(set) = seen.get_mut(&id) {
        set.remove(&protocol);
        if set.is_empty() {
            seen.remove(&id);
            emit(&c.sink, Event::Removed(id))
        }
    }
}
unsafe extern "C" fn resolve_cb(
    _: P,
    interface: i32,
    _: i32,
    event: i32,
    _: *const c_char,
    _: *const c_char,
    _: *const c_char,
    host: *const c_char,
    address: P,
    port: u16,
    txt: P,
    _: u32,
    data: P,
) {
    let c = unsafe { &*data.cast::<Context>() };
    if event != 0 {
        return;
    }
    let Request::Resolve(id) = &c.request else {
        return;
    };
    let mut addresses = c.addresses.borrow_mut();
    let mut id = id.clone();
    id.interface = interface.max(0) as u32;
    let mut buffer = [0 as c_char; 128];
    unsafe { (c.api.avahi_address_snprint)(buffer.as_mut_ptr(), buffer.len(), address) };
    if let Ok(ip) = unsafe { s(buffer.as_ptr()) }.parse::<std::net::IpAddr>() {
        let addr = match ip {
            std::net::IpAddr::V6(ip) => SocketAddr::V6(std::net::SocketAddrV6::new(
                ip,
                port,
                0,
                if ip.is_unicast_link_local() {
                    id.interface
                } else {
                    0
                },
            )),
            _ => SocketAddr::new(ip, port),
        };
        addresses.retain(|a| a.is_ipv4() != addr.is_ipv4());
        addresses.push(addr);
    }
    let len = unsafe { (c.api.avahi_string_list_serialize)(txt, ptr::null_mut(), 0) };
    if len > 65535 {
        emit(&c.sink, Event::Error(Error::Overflow));
        return;
    }
    let mut bytes = vec![0; len];
    unsafe { (c.api.avahi_string_list_serialize)(txt, bytes.as_mut_ptr(), len) };
    if let Ok(txt) = TxtRecord::decode(&bytes) {
        emit(
            &c.sink,
            Event::Resolved(Service {
                id,
                hostname: unsafe { s(host) },
                port,
                addresses: addresses.clone(),
                txt,
            }),
        )
    }
}
unsafe extern "C" fn group_cb(_: P, state: i32, data: P) {
    let c = unsafe { &*data.cast::<Context>() };
    if let Request::Advertise(a) = &c.request {
        if state == 2 {
            emit(
                &c.sink,
                Event::Registered(ServiceId {
                    name: a.name.clone(),
                    service_type: a.service_type.clone(),
                    domain: "local.".into(),
                    interface: a.interface,
                }),
            )
        } else if state == 3 || state == 4 {
            emit(&c.sink, Event::Error(failure(state)))
        }
    }
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
pub fn start(request: Request, sink: Sink) -> Result<Box<dyn Native>> {
    let api = Arc::new(Api::load()?);
    let stop = Arc::new(AtomicBool::new(false));
    let cancelled = stop.clone();
    let t = thread::Builder::new()
        .name("avahi".into())
        .spawn(move || {
            let mut ctx = Box::new(Context {
                api: api.clone(),
                sink,
                request,
                seen: Default::default(),
                addresses: Default::default(),
            });
            let data = (&mut *ctx as *mut Context).cast();

            let poll = unsafe { (api.avahi_simple_poll_new)() };
            if poll.is_null() {
                emit(&ctx.sink, Event::Error(failure(-1)));
                return;
            }
            let mut error = 0;
            let client = unsafe {
                (api.avahi_client_new)(
                    (api.avahi_simple_poll_get)(poll),
                    0,
                    client_cb,
                    data,
                    &mut error,
                )
            };
            if client.is_null() {
                emit(
                    &ctx.sink,
                    Event::Error(Error::Unavailable(format!("Avahi daemon: {error}"))),
                );
                unsafe { (api.avahi_simple_poll_free)(poll) };
                return;
            }
            // Avahi can call back during client construction. Wait until registration is allowed.
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            while unsafe { (api.avahi_client_get_state)(client) } != 2
                && !cancelled.load(Ordering::Acquire)
                && std::time::Instant::now() < deadline
            {
                unsafe { (api.avahi_simple_poll_iterate)(poll, 100) };
            }
            let cs = |s: &str| {
                CString::new(s).map_err(|_| Error::InvalidInput("NUL in DNS name".into()))
            };
            let result = (|| -> Result<()> {
                unsafe {
                    match &ctx.request {
                        Request::Browse(t, i) => {
                            let i = if *i == 0 { -1 } else { *i as i32 };
                            let p = if t == ENUMERATION_TYPE {
                                (api.avahi_service_type_browser_new)(
                                    client,
                                    i,
                                    -1,
                                    ptr::null(),
                                    0,
                                    types_cb,
                                    data,
                                )
                            } else {
                                let t = cs(t)?;
                                (api.avahi_service_browser_new)(
                                    client,
                                    i,
                                    -1,
                                    t.as_ptr(),
                                    ptr::null(),
                                    0,
                                    browse_cb,
                                    data,
                                )
                            };
                            if p.is_null() {
                                return Err(failure((api.avahi_client_errno)(client)));
                            }
                        }
                        Request::Resolve(id) => {
                            let n = cs(&id.name)?;
                            let t = cs(&id.service_type)?;
                            let d = cs(&id.domain)?;
                            for family in [0, 1] {
                                let r = (api.avahi_service_resolver_new)(
                                    client,
                                    if id.interface == 0 {
                                        -1
                                    } else {
                                        id.interface as i32
                                    },
                                    -1,
                                    n.as_ptr(),
                                    t.as_ptr(),
                                    d.as_ptr(),
                                    family,
                                    0,
                                    resolve_cb,
                                    data,
                                );
                                if r.is_null() {
                                    return Err(failure((api.avahi_client_errno)(client)));
                                }
                            }
                        }
                        Request::Advertise(a) => {
                            let group = (api.avahi_entry_group_new)(client, group_cb, data);
                            if group.is_null() {
                                return Err(failure(-1));
                            }
                            let n = cs(&a.name)?;
                            let t = cs(&a.service_type)?;
                            let mut list = ptr::null_mut();
                            let bytes = a.txt.encode()?;
                            let mut rest = bytes.as_slice();
                            while let Some((&n, tail)) = rest.split_first() {
                                list = (api.avahi_string_list_add_arbitrary)(
                                    list,
                                    tail.as_ptr(),
                                    n as usize,
                                );
                                rest = &tail[n as usize..];
                            }
                            let code = (api.avahi_entry_group_add_service_strlst)(
                                group,
                                if a.interface == 0 {
                                    -1
                                } else {
                                    a.interface as i32
                                },
                                -1,
                                0,
                                n.as_ptr(),
                                t.as_ptr(),
                                ptr::null(),
                                ptr::null(),
                                a.port,
                                list,
                            );
                            (api.avahi_string_list_free)(list);
                            if code < 0 {
                                return Err(failure(code));
                            }
                            let code = (api.avahi_entry_group_commit)(group);
                            if code < 0 {
                                return Err(failure(code));
                            }
                        }
                    }
                    Ok(())
                }
            })();
            if let Err(e) = result {
                emit(&ctx.sink, Event::Error(e))
            } else {
                while !cancelled.load(Ordering::Acquire) {
                    if unsafe { (api.avahi_simple_poll_iterate)(poll, 100) } < 0 {
                        emit(&ctx.sink, Event::Error(failure(-1)));
                        break;
                    }
                }
            }
            // Free the client and all of its child objects while callback context and libraries live.
            unsafe {
                (api.avahi_client_free)(client);
                (api.avahi_simple_poll_free)(poll)
            }
        })
        .map_err(|e| Error::Unavailable(e.to_string()))?;
    Ok(Box::new(Handle {
        stop,
        thread: Some(t),
    }))
}
