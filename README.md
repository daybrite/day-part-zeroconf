# day-part-zeroconf

Native DNS-SD discovery, resolution and advertisement for Day apps. External Day Part,
following `day-part-speech`; MPL-2.0. No Tokio, multicast responder, bundled daemon, or dependency
on `mdns-sd`, `astro-dnssd`, `zeroconf`, or `mdns-sd-discovery`.

The **Zeroconf Explorer** demo is a network workbench: browse service types, filter by name/type/
host, inspect each interface observation and its addresses and TXT data, publish an endpoint,
and try TCP or HTTP connections. An optional built-in HTTP server makes a complete roundtrip
possible in one app. Navigation adapts between desktop columns and mobile list/detail screens.
App text is in the Fluent catalog, including permission descriptions and connection results.

## API

```rust,no_run
use day_part_zeroconf::{browse, BrowseOptions, Event};
use std::time::Duration;

let mut discovery = browse(BrowseOptions::service("_calibre._tcp"))?;
while let Some(event) = discovery.recv_timeout(Duration::from_secs(10)) {
    if let Event::Resolved(service) = event {
        println!("{}: {:?}", service.id.name, service.addresses);
        // Calibre advertises its OPDS location as the TXT attribute `path`.
        println!("path: {:?}", service.txt.get("path"));
    }
}
discovery.stop();
# Ok::<(), day_part_zeroconf::Error>(())
```

Use `BrowseOptions::all()` to enumerate types and browse each automatically. Use `resolve(id)`
for an identity already known to the application, and `advertise(Advertisement { … })` for a
listener that the application owns. `Registered` carries the actual instance name after native
conflict handling. Linux reports a collision rather than renaming automatically.

`operation.next_event().await` works with any executor. `recv_timeout` is for worker threads.
`operation.cancellation().cancel()` is nonblocking and suitable for UI actions. Dropping an
operation cancels it; `stop()` also joins its coordinator. Native mobile cancellation can
finish asynchronously. Keep the operation alive for as long as browsing or advertising is wanted.

Observations are keyed by `(name, type, domain, interface)`. A name is not an authenticated device
identity. Preserve the scope ID on IPv6 link-local addresses. `Resolved` can arrive several times
as addresses become available. `Removed` invalidates that observation; queued callbacks from an
older resolver are discarded. Windows, older Android, and Harmony resolution is one-shot:
refresh browsing or resolve again to guarantee fresh metadata. The API currently covers `.local`
service types (`_name._tcp` / `_name._udp`), not wide-area DNS-SD or subtype management.

TXT values are bytes. `None` means a key-only attribute and differs from `Some(vec![])`. Keys are
case-insensitive, duplicates are rejected when publishing, and parsing keeps the first valid
occurrence. Publishing limits each item to 255 bytes and the complete record to 1300 bytes.
Check `capabilities()` before relying on binary publication, key-only publication, or explicit
interface selection. Unsupported publication formats are rejected rather than coerced.

The queues are bounded (1024 events/observations, 128 types). On overflow, stop the operation and
restart with a narrower type. Callbacks never block on the consumer or call application code.
Each native operation owns its callback storage until the native API can no longer access it.

Discovery does **not** open arbitrary discovered services, implement a transport, pair devices,
or establish trust. Use `std::net`, Day's HTTP part, or your application protocol after an explicit
user action. The demo's TCP probe opens and closes a connection; HTTP sends a bounded plain-HTTP
GET for `_http`, `_calibre`, and its own `_dayzc` service. TLS and authentication belong to the
application. TXT `path` data cannot inject headers or redirect the probe to another endpoint.

## Native implementations

| Platform | Browse / resolve / advertise | Important differences |
| --- | --- | --- |
| macOS, iOS | Bonjour `DNSServiceBrowse`, `Resolve`, `GetAddrInfo`, `Register` in libSystem | Binary TXT; native interface indices; iOS all-types entitlement |
| Linux (GTK, Qt) | Avahi client/simple-poll/entry-group APIs | Dynamically loads `libavahi-client.so.3` and `libavahi-common.so.3`; requires the Avahi daemon/system D-Bus |
| Windows 10+ (WinUI, XAML, GTK, Qt) | `DnsServiceBrowse`, `Resolve`, `Register`, `DeRegister` in dnsapi.dll | No Bonjour install; UTF-8 key=value publication only, no embedded NUL; one IPv4 and IPv6 address exposed by this API |
| Android | `NsdManager`; continuous service-info callbacks on API 34+, serialized legacy resolution earlier | UTF-8 TXT publication via the public string API; binary values preserved on receive; OS chooses network; hostname available on API 36+ |
| HarmonyOS / OpenHarmony | `@kit.NetworkKit` mDNS service through Daybridge | Binary attribute values; rejects key-only publication; OS chooses interface and returns one address |
| Web DOM / other targets | Explicit `Unsupported` | Browsers expose no general LAN DNS-SD API |

On Android and HarmonyOS, all-types browsing uses a small DNS-SD **type query** because their
native discovery APIs do not reliably expose the meta-enumeration operation. The query uses an
ephemeral UDP socket, requests unicast answers, bounds DNS compression parsing, and then hands
each resulting type to the native browser. It uses the default IPv4 multicast route; it does not
enumerate every interface or IPv6-only network. Targeted native browsing remains available on
those networks. Responders that omit service-type enumeration (including OpenHarmony’s native
responder) need a targeted query; an empty all-types scan does not prove that no services exist.
This fallback is deliberately not a second mDNS implementation: no responder,
address resolver, registration, probing, or TTL cache. See `src/enumerate.rs` and its packet tests.

## Permissions and host setup

* **iOS:** declare the specific types in `NSBonjourServices` and a localized
  `NSLocalNetworkUsageDescription`. The demo declares `_dayzc`, HTTP(S), Calibre, OPDS, SSH, SMB,
  and IPP. Browsing arbitrary types on a physical device requires Apple's approved
  `com.apple.developer.networking.multicast` entitlement and a matching provisioning profile;
  adding a plist entry cannot grant it. The demo's default signing does not claim that entitlement.
* **macOS:** allow local-network access when prompted. Sandboxed apps need the appropriate
  network client/server entitlements for browsing and their own listener.
* **Android:** part metadata contributes Internet/network/multicast permissions. On Android 17+
  when targeting API 37+, declare and request `android.permission.ACCESS_LOCAL_NETWORK` before
  discovery. `requires_explicit_permission()` identifies this case; the demo uses
  `day-part-permissions`. A multicast lock is acquired only on older Android releases and released
  on cancellation. Build native glue with SDK 36 or later; older runtime APIs are guarded.
* **HarmonyOS:** `ohos.permission.INTERNET` is required and is already in Day's generated host.
  Do not repeat it in dependency contributions; hvigor rejects duplicate entries.
* **Linux:** install/start Avahi. Flatpak apps need network access and system-bus access to
  `org.freedesktop.Avahi` (the part contributes the latter). No private daemon is started.
* **Windows:** allow mDNS and the application's listener through the firewall as appropriate.

## Demo and tests

```sh
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo test --test native -- --ignored --test-threads=1
cargo test --lib enumerate::tests::native_legacy_query -- --ignored
cd demo
cargo test
cargo clippy --all-targets -- -D warnings
day lint --strict
day launch -t macos-appkit --script dayscript/demo.yaml
# ios-uikit, android-mdc, linux-gtk, linux-qt, windows-winui, harmony-arkui
```

`dayscript/demo.yaml` exercises real publish → browse → resolve → TCP → HTTP → withdraw.
`dayscript/enumeration.yaml` additionally exercises type enumeration; physical iOS needs the
entitlement and is excluded from its default CI run. HarmonyOS runs the targeted workflow:
its responder does not announce its own type through meta-enumeration. The shared query path
is covered by packet tests, the native responder test, and Android’s enumeration workflow. OS permission dialogs and cross-device
network isolation still need manual checks. HarmonyOS emulator testing belongs in CI, not on
this ARM Mac.

`.github/workflows/ci.yml` runs host tests/lints on Linux, macOS and Windows, checks mobile Rust
and the explicit web fallback, and uses Day's shared app workflow for all seven demo targets.
It starts Avahi on Linux and configures firewall rules on disposable Windows runners. The
framework is tracked by its canonical Git URL, like `day-part-speech`; generated local patches
and Cargo locks are ignored. CI tracks Day's current main and runs daily as well as on changes.

Local validation on 2026-10-14: 13 core unit tests, two demo unit tests, and three opt-in native
tests passed. Demo walkthroughs passed on macOS (47 steps), the iOS simulator (35), and Android
(48). The HarmonyOS Rust/ArkTS app built and packaged successfully. Rust/Clippy checks passed
for Windows, Linux, iOS, Android, HarmonyOS and the unsupported web backend. Windows/Linux
native execution and HarmonyOS emulator execution remain for CI; the workflow has not yet run.

Two Day Android UI integration issues remain visible in the current framework checkout:
navigation SVGs appear as circular placeholders, and a busy enumeration can produce a
nonfatal `RecyclerView` reload-during-layout exception in `DayBridge.listReload`. The discovery
and connection walkthroughs pass despite these; the framework itself is not modified here.

## Source study and design decisions

Reviewed source archives: **mdns-sd 0.21.5**, **astro-dnssd 0.3.6**, **zeroconf 0.18.0**, and
**mdns-sd-discovery 0.3.3**. This implementation was written independently; no code was vendored.

* [mdns-sd](https://github.com/keepsimple1/mdns-sd): its daemon/cache/socket design covers probing,
  interface changes, TTL expiry, and conflicts. That breadth is valuable for systems without a
  native daemon, but duplicates the OS stack on the requested platforms.
* [astro-dnssd](https://github.com/AstroHQ/astro-dnssd): studied native browse/register and callback
  ownership. A thin wrapper is useful, but does not supply the Android/Harmony lifecycle needed
  here. Our Windows requests retain callback storage through completion, including cancellation.
* [zeroconf](https://github.com/windy1/zeroconf-rs): studied Bonjour, Avahi client/resolver and
  entry-group integration. Using it would still require separate mobile and native Windows
  implementations and impose its native dependency choices.
* [mdns-sd-discovery](https://github.com/abbyssoul/mdns_sd_discovery): the closest native abstraction;
  studied its platform modules and Linux D-Bus implementation. Its async/D-Bus dependencies and
  discovery scope do not replace the registration and Daybridge work here.

There was no compelling dependency advantage for this scope. Runtime dependencies are only
platform bindings (`libc`, `libloading`, `windows-sys`) and Daybridge/JNI on mobile. `day-build`
is a build dependency. Connections stay outside the part so HTTP libraries, game protocols,
file sharing, and smart-device pairing can share discovery without sharing transport policy.

Primary references: [Apple DNS-SD](https://developer.apple.com/documentation/dnssd),
[Apple local-network privacy](https://developer.apple.com/documentation/technotes/tn3179-understanding-local-network-privacy),
[Android NSD](https://developer.android.com/develop/connectivity/wifi/use-nsd),
[Windows DNS-SD](https://learn.microsoft.com/en-us/windows/win32/api/windns/nf-windns-dnsservicebrowse),
[Windows cancellation](https://learn.microsoft.com/en-us/windows/win32/api/windns/nf-windns-dnsservicebrowsecancel),
[Avahi client API](https://avahi.org/doxygen/client_8h.html),
[OpenHarmony mDNS](https://github.com/openharmony/docs/blob/master/en/application-dev/reference/apis-network-kit/js-apis-net-mdns.md),
[OpenHarmony type validation](https://github.com/openharmony/communication_netmanager_ext/blob/master/frameworks/native/mdnsclient/src/mdns_common.cpp),
and [RFC 6762](https://www.rfc-editor.org/rfc/rfc6762.html) /
[RFC 6763](https://www.rfc-editor.org/rfc/rfc6763.html).

Stanza-Redux integration is phase 2; this project does not change it.
