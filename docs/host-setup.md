<!--
Copyright © The Daybrite Project
SPDX-License-Identifier: MPL-2.0
-->

# Permissions and host setup

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


See the [service-discovery guide](https://daybrite.dev/docs/reference/zeroconf/) for the API and lifecycle.
