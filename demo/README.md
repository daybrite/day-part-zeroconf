# Zeroconf Explorer

A native Day workbench for [day-part-zeroconf](../README.md): browse the local network,
search by instance name, service type or hostname, inspect addresses and TXT attributes,
publish a service, and test TCP or plain-HTTP connections.

## Run

```sh
day launch -t macos-appkit
# Also: linux-gtk, linux-qt, windows-winui, ios-uikit, android-material, harmony-arkui
```

Leave the service type blank to enumerate types, or enter a type such as `_calibre._tcp`.
The search field filters discovered services locally. Select a service to inspect its resolved
endpoint; connection buttons perform explicit network requests. UDP services can be inspected
and advertised but do not have a generic connection probe.

The Publish page advertises an existing listener when a port is supplied. Leave the port blank
to start the built-in HTTP test server, using a TCP service type. TXT entries accept `key=value`
or a key-only line, subject to the platform's capabilities. Withdrawing closes the test server.
Settings provides appearance and language preferences. Navigation uses columns on desktop and
list/detail screens on phones. Service rows use consistent leading alignment and colored network
badges. The inspector keeps connection actions near the top, with selectable monospace panels
for addresses, TXT records, responses and activity. Long lines scroll horizontally; larger
blocks also scroll vertically. Light and dark appearances use native surfaces around the
high-contrast data panels.

See the parent README for native dependencies, permissions, iOS entitlement requirements,
mobile enumeration limits and the differences in TXT support. No app-store submission lanes
are configured; local builds and CI produce development artifacts.

## Validation

```sh
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
day lint --strict
day launch -t macos-appkit --script dayscript/demo.yaml --script dayscript/enumeration.yaml --script dayscript/presentation.yaml
```

`demo.yaml` performs real advertisement, targeted discovery, resolution, TCP and HTTP requests,
and withdrawal. `enumeration.yaml` tests all-types discovery. The latter is excluded from the
default iOS and HarmonyOS CI runs for the platform reasons documented in the parent README.
`presentation.yaml` publishes long synthetic names and TXT values through the native stack,
captures the browser/inspector and scrolls both axes of the data panel. CI runs it on every demo
target. Screenshots and generated native projects are under `build/`.

## Code

- `src/lib.rs`: adaptive navigation and window configuration.
- `src/model.rs`: discovery/advertisement lifetimes, selection, search and event state.
- `src/pages/`: browser, inspector, publisher and settings views.
- `src/network.rs`: bounded connection probes and the small test HTTP server.
- `resource/locales/en/app.ftl`: app-owned copy and permission messages.
- `platform/`: app-owned native host sources, configuration and permission declarations.

Bare Cargo commands use Day's mock UI backend. Use `day launch` to exercise native discovery
inside a real app; the mock unit tests cover the TXT editor and actual loopback TCP/HTTP traffic.
