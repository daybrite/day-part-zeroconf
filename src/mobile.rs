// Copyright © The Daybrite Project
// SPDX-License-Identifier: MPL-2.0
//! Platform NSD adapters. Binary frames preserve TXT bytes without a serialization dependency.
use super::*;
fn bridge_error(e: day_bridge::Error) -> Error {
    match e {
        day_bridge::Error::Unsupported => Error::Unsupported,
        _ => Error::Unavailable(e.to_string()),
    }
}
pub fn capabilities() -> Capabilities {
    Capabilities {
        native: true,
        browse_all: true,
        binary_txt: cfg!(target_env = "ohos"),
        key_only_txt: !cfg!(target_env = "ohos"),
        interface_selection: false,
    }
}
struct Handle(u64);
impl Drop for Handle {
    fn drop(&mut self) {
        operation_native_stop(self.0);
        cancel_native(self.0 as i64);
    }
}
pub fn requires_explicit_permission() -> bool {
    permission_required_native().unwrap_or(false)
}
pub fn start(request: Request, sink: Sink) -> Result<Box<dyn Native>> {
    if matches!(&request,Request::Browse(t,0) if t==ENUMERATION_TYPE) {
        return crate::enumerate::start(sink);
    }
    #[cfg(target_os = "android")]
    if let Request::Advertise(a) = &request {
        for (_, v) in &a.txt.0 {
            if let Some(v) = v
                && std::str::from_utf8(v).is_err()
            {
                return Err(Error::Unsupported);
            }
        }
    }
    #[cfg(target_env = "ohos")]
    if let Request::Advertise(a) = &request
        && a.txt.0.iter().any(|(_, v)| v.is_none())
    {
        return Err(Error::Unsupported);
    }
    let (mode, name, kind, domain, port, txt, interface) = match request {
        Request::Browse(t, i) => (0, String::new(), t, "local.".into(), 0, vec![], i),
        Request::Resolve(id) => (
            1,
            id.name,
            id.service_type,
            id.domain,
            0,
            vec![],
            id.interface,
        ),
        Request::Advertise(a) => (
            2,
            a.name,
            a.service_type,
            "local.".into(),
            i32::from(a.port),
            a.txt.encode()?,
            a.interface,
        ),
    };
    if interface != 0 {
        return Err(Error::Unsupported);
    }
    let token =
        operation_native_stream(
            mode,
            &name,
            &kind,
            &domain,
            port,
            &txt,
            move |item| match item {
                day_bridge::Item::Value(bytes) => match decode(&bytes) {
                    Ok(event) => emit(&sink, event),
                    Err(e) => emit(&sink, Event::Error(e)),
                },
                day_bridge::Item::Failed(e) => emit(&sink, Event::Error(bridge_error(e))),
                day_bridge::Item::End => {}
            },
        )
        .map_err(bridge_error)?;
    Ok(Box::new(Handle(token)))
}
use crate::wire::decode;

day_bridge::bridge! {
 #[day_bridge::declare]
 extern "day" {
  fn operation_native(mode:i32,name:&str,kind:&str,domain:&str,port:i32,txt:&[u8],events:day_bridge::Emit<Vec<u8>>)->Result<(),day_bridge::Error>;
  fn cancel_native(token:i64);
  fn permission_required_native()->Result<bool,day_bridge::Error>;
 }
 #[day_bridge::impl(java, platforms=[android])]
 java!(prelude=r#"
 import android.content.Context;
 import android.net.nsd.NsdManager;
 import android.net.nsd.NsdServiceInfo;
 import android.net.wifi.WifiManager;
 import android.os.Build;
 import java.net.InetAddress;
 import java.io.ByteArrayOutputStream;
 import java.io.DataOutputStream;
 import java.nio.charset.StandardCharsets;
 import java.util.ArrayDeque;
 import java.util.concurrent.ConcurrentHashMap;
 import dev.daybrite.day.bridge.DayBridge;
 "#,body=r#"
 private static final ConcurrentHashMap<Long, Op> ops = new ConcurrentHashMap<>();
 private static final ArrayDeque<Runnable> resolveQueue = new ArrayDeque<>();
 private static boolean resolving;
 private static synchronized void resolveLater(Runnable task) { resolveQueue.add(task); if (!resolving) resolveNext(); }
 private static synchronized void resolveNext() { Runnable next = resolveQueue.poll(); resolving = next != null; if (next != null) next.run(); }
 private static class Frame {
  final ByteArrayOutputStream bytes = new ByteArrayOutputStream(); final DataOutputStream out = new DataOutputStream(bytes);
  Frame(int tag) { num(tag); }
  Frame num(int n) { try { out.writeInt(n); } catch (java.io.IOException e) { throw new IllegalStateException(e); } return this; }
  Frame raw(byte[] b) { num(b.length); try { out.write(b); } catch (java.io.IOException e) { throw new IllegalStateException(e); } return this; }
  Frame str(String s) { return raw((s == null ? "" : s).getBytes(StandardCharsets.UTF_8)); }
 }
 private static class Op {
  final long token; final NsdManager manager; volatile boolean stopped; NsdManager.DiscoveryListener browse; NsdManager.RegistrationListener registration; NsdManager.ServiceInfoCallback updates; WifiManager.MulticastLock lock;
  Op(long token) { this.token = token; manager = (NsdManager)DayBridge.ctx.getSystemService(Context.NSD_SERVICE); }
  void send(Frame f) { if (!stopped) operation_native_emit(token, f.bytes.toByteArray()); }
  void error(int code, String operation) { send(new Frame(6).num(code).str(operation)); }
  Frame identity(int tag, NsdServiceInfo s) { return new Frame(tag).str(s.getServiceName()).str(s.getServiceType()).str("local."); }
  void resolved(NsdServiceInfo s) {
   java.util.List<InetAddress> addresses;
   if (Build.VERSION.SDK_INT >= 34) addresses = s.getHostAddresses();
   else addresses = s.getHost() == null ? java.util.Collections.emptyList() : java.util.Collections.singletonList(s.getHost());
   String hostname = Build.VERSION.SDK_INT >= 36 ? s.getHostname() : null;
   Frame f = identity(3,s).str(hostname == null ? "" : hostname+".local.").num(s.getPort()).num(addresses.size());
   for (InetAddress a: addresses) { String text=a.getHostAddress(); if(a instanceof java.net.Inet6Address) {int at=text.indexOf('%'); if(at>=0)text=text.substring(0,at);int scope=((java.net.Inet6Address)a).getScopeId();if(scope!=0)text+="%"+scope;} f.str(text); }
   ByteArrayOutputStream txt = new ByteArrayOutputStream();
   for (java.util.Map.Entry<String,byte[]> e: s.getAttributes().entrySet()) {
    byte[] k = e.getKey().getBytes(StandardCharsets.UTF_8), v = e.getValue(); int length = k.length + (v == null ? 0 : 1+v.length);
    if (length > 255) continue; txt.write(length); txt.write(k,0,k.length); if(v != null) {txt.write('=');txt.write(v,0,v.length);}
   }
   send(f.raw(txt.toByteArray()));
  }
 }
 public static boolean permission_required_native() { return Build.VERSION.SDK_INT >= 37 && DayBridge.ctx.getApplicationInfo().targetSdkVersion >= 37; }
 public static void operation_native(int mode,String name,String kind,String domain,int port,byte[] txt,long events) {
  Op op = new Op(events); ops.put(events,op);
  try {
   if (Build.VERSION.SDK_INT >= 37 && DayBridge.ctx.getApplicationInfo().targetSdkVersion >= 37 && DayBridge.ctx.checkSelfPermission("android.permission.ACCESS_LOCAL_NETWORK") != android.content.pm.PackageManager.PERMISSION_GRANTED) { op.error(-1,"ACCESS_LOCAL_NETWORK"); return; }
   WifiManager wifi = (WifiManager)DayBridge.ctx.getApplicationContext().getSystemService(Context.WIFI_SERVICE);
   if (wifi != null && Build.VERSION.SDK_INT < 34) { op.lock = wifi.createMulticastLock("day-zeroconf"); op.lock.setReferenceCounted(false); op.lock.acquire(); }
   if (mode == 0) {
    op.browse = new NsdManager.DiscoveryListener() {
     public void onDiscoveryStarted(String t) {}
     public void onDiscoveryStopped(String t) {}
     public void onStartDiscoveryFailed(String t,int code) { op.error(code,"discover"); }
     public void onStopDiscoveryFailed(String t,int code) { op.error(code,"stop discovery"); }
     public void onServiceFound(NsdServiceInfo s) {
      op.send(op.identity(1,s));
     }
     public void onServiceLost(NsdServiceInfo s) { op.send(op.identity(2,s)); }
    }; op.manager.discoverServices(kind,NsdManager.PROTOCOL_DNS_SD,op.browse);
   } else if (mode == 1) {
    NsdServiceInfo service = new NsdServiceInfo(); service.setServiceName(name); service.setServiceType(kind);
    if (Build.VERSION.SDK_INT >= 34) {
     op.updates = new NsdManager.ServiceInfoCallback() {
      public void onServiceInfoCallbackRegistrationFailed(int code) { op.error(code,"resolve"); }
      public void onServiceUpdated(NsdServiceInfo info) { op.resolved(info); }
      public void onServiceLost() { op.send(op.identity(2,service)); }
      public void onServiceInfoCallbackUnregistered() {}
     }; op.manager.registerServiceInfoCallback(service,Runnable::run,op.updates);
    } else resolveLater(()-> {
     if (op.stopped) {resolveNext();return;}
     try {op.manager.resolveService(service,new NsdManager.ResolveListener() {
      public void onResolveFailed(NsdServiceInfo s,int code) {op.error(code,"resolve");resolveNext();}
      public void onServiceResolved(NsdServiceInfo s) {op.resolved(s);resolveNext();}
     });} catch (RuntimeException e) {op.error(-2,e.toString());resolveNext();}
    });
   } else {
    NsdServiceInfo service = new NsdServiceInfo(); service.setServiceName(name); service.setServiceType(kind);service.setPort(port);
    for(int at=0;at<txt.length;) {int length=txt[at++]&255;int end=at+length;if(end>txt.length)throw new IllegalArgumentException("TXT length");int split=at;while(split<end&&txt[split]!='=')split++;String key=new String(txt,at,split-at,StandardCharsets.UTF_8);service.setAttribute(key,split==end?(String)null:new String(txt,split+1,end-split-1,StandardCharsets.UTF_8));at=end;}
    op.registration = new NsdManager.RegistrationListener() {
     public void onServiceRegistered(NsdServiceInfo s) {op.send(new Frame(4).str(s.getServiceName()).str(kind).str("local."));}
     public void onRegistrationFailed(NsdServiceInfo s,int code) {op.error(code,"register");}
     public void onServiceUnregistered(NsdServiceInfo s) {}
     public void onUnregistrationFailed(NsdServiceInfo s,int code) {op.error(code,"unregister");}
    };op.manager.registerService(service,NsdManager.PROTOCOL_DNS_SD,op.registration);
   }
  } catch(SecurityException e) {op.error(-1,e.toString());} catch(RuntimeException e) {op.error(-2,e.toString());}
 }
 public static void cancel_native(long token) {
  Op op=ops.remove(token);if(op==null)return;op.stopped=true;
  try {if(op.browse!=null)op.manager.stopServiceDiscovery(op.browse);}catch(RuntimeException ignored){}
  try {if(op.registration!=null)op.manager.unregisterService(op.registration);}catch(RuntimeException ignored){}
  try {if(op.updates!=null&&Build.VERSION.SDK_INT>=34)op.manager.unregisterServiceInfoCallback(op.updates);}catch(RuntimeException ignored){}
  if(op.lock!=null&&op.lock.isHeld())op.lock.release();operation_native_end(token);
 }
 "#);
 #[day_bridge::impl(arkts, platforms=[ohos])]
 arkts!(prelude=r#"
 import { mdns } from '@kit.NetworkKit';
 import { common } from '@kit.AbilityKit';
 import { util } from '@kit.ArkTS';
 import { BusinessError } from '@kit.BasicServicesKit';
 "#,body=r#"
 class Frame {
  parts: Array<Uint8Array> = [];
  constructor(tag: number) {this.num(tag);}
  num(n: number): Frame {const b=new Uint8Array(4);new DataView(b.buffer).setUint32(0,n,false);this.parts.push(b);return this;}
  raw(b: Uint8Array): Frame {this.num(b.length);this.parts.push(b);return this;}
  str(s: string): Frame {return this.raw(s.length===0?new Uint8Array(0):new util.TextEncoder().encodeInto(s));}
  done(): Uint8Array {let size=0;for(const p of this.parts)size+=p.length;const b=new Uint8Array(size);let at=0;for(const p of this.parts){b.set(p,at);at+=p.length;}return b;}
 }
 class Op {token: number=0; stopped: boolean=false; discovery: mdns.DiscoveryService|undefined=undefined; service: mdns.LocalServiceInfo|undefined=undefined; registered: boolean=false; context: common.UIAbilityContext; constructor(token:number,context:common.UIAbilityContext){this.token=token;this.context=context;}}
 const ops: Map<number,Op> = new Map<number,Op>();
 function send(op:Op,f:Frame):void{if(!op.stopped)operation_native_emit(op.token,f.done());}
 function identity(tag:number,s:mdns.LocalServiceInfo):Frame{return new Frame(tag).str(s.serviceName).str(s.serviceType).str('local.');}
 function failed(op:Op,e:BusinessError):void{send(op,new Frame(6).num(e.code).str(e.message));}
 function resolved(op:Op,s:mdns.LocalServiceInfo):void {
  const host=s.host?.address??'';const f=identity(3,s).str(host).num(s.port).num(host.length>0?1:0);if(host.length>0)f.str(host);
  const bytes:Array<number>=[];if(s.serviceAttribute){for(const a of s.serviceAttribute){const k=new util.TextEncoder().encodeInto(a.key);const v=a.value;const n=k.length+1+v.length;if(n>255)continue;bytes.push(n);for(const b of k)bytes.push(b);bytes.push(61);for(const b of v)bytes.push(b);}}
  send(op,f.raw(new Uint8Array(bytes)));
 }
 export function permission_required_native():boolean {return false;}
 export function operation_native(mode:number,name:string,kind:string,domain:string,port:number,txt:Uint8Array,events:number):void {
  const context=getContext() as common.UIAbilityContext;const op=new Op(events,context);ops.set(events,op);
  try {
   if(mode===0){
    const discovery=mdns.createDiscoveryService(context,kind);op.discovery=discovery;
    discovery.on('serviceFound',(s:mdns.LocalServiceInfo)=>{send(op,identity(1,s));});
    discovery.on('serviceLost',(s:mdns.LocalServiceInfo)=>{send(op,identity(2,s));});
    discovery.on('discoveryStart',(event:mdns.DiscoveryEventInfo)=>{if(event.errorCode)send(op,new Frame(6).num(event.errorCode).str('discoveryStart'));});
    discovery.startSearchingMDNS();
   }else if(mode===1){const s:mdns.LocalServiceInfo={serviceName:name,serviceType:kind,port:0};mdns.resolveLocalService(context,s).then((info:mdns.LocalServiceInfo)=>resolved(op,info)).catch((e:BusinessError)=>failed(op,e));
   }else{
    const attrs:Array<mdns.ServiceAttribute>=[];let at=0;while(at<txt.length){const n=txt[at++];const end=at+n;if(end>txt.length)throw new Error('TXT length');let split=at;while(split<end&&txt[split]!==61)split++;const key=new util.TextDecoder('utf-8').decodeToString(txt.slice(at,split));const value:Array<number>=[];for(let j=split+1;j<end;j++)value.push(txt[j]);attrs.push({key:key,value:value});at=end;}
    const s:mdns.LocalServiceInfo={serviceName:name,serviceType:kind,port:port,serviceAttribute:attrs};op.service=s;
    mdns.addLocalService(context,s).then((info:mdns.LocalServiceInfo)=>{op.service=info;op.registered=true;if(op.stopped){mdns.removeLocalService(context,info).catch((_e:BusinessError)=>{});}else send(op,identity(4,info));}).catch((e:BusinessError)=>failed(op,e));
   }
  }catch(e){failed(op,e as BusinessError);}
 }
 export function cancel_native(token:number):void {
  const op=ops.get(token);if(!op)return;ops.delete(token);op.stopped=true;
  if(op.discovery){try{op.discovery.stopSearchingMDNS();op.discovery.off('serviceFound');op.discovery.off('serviceLost');op.discovery.off('discoveryStart');}catch(_e){}}
  if(op.service&&op.registered)mdns.removeLocalService(op.context,op.service).catch((_e:BusinessError)=>{});
  operation_native_end(token);
 }
 "#);
 #[day_bridge::impl(rust, platforms=[other])]
 fn operation_native(_mode:i32,_name:&str,_kind:&str,_domain:&str,_port:i32,_txt:&[u8],_events:day_bridge::Emit<Vec<u8>>)->Result<(),day_bridge::Error>{Err(day_bridge::Error::Unsupported)}
 #[day_bridge::impl(rust, platforms=[other])]
 fn cancel_native(_token:i64){}
 #[day_bridge::impl(rust, platforms=[other])]
 fn permission_required_native()->Result<bool,day_bridge::Error>{Ok(false)}
}
