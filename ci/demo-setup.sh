# Sourced by Day's shared workflow from demo/, immediately before its dayscripts.
# This never changes a developer machine's permission state.
if [[ ${GITHUB_ACTIONS:-} == true && ${DAY_SCRIPT_TARGET:-} == macos-appkit ]]; then
  osascript ../ci/allow-local-network.applescript > "$RUNNER_TEMP/zeroconf-permission.log" 2>&1 &
  zeroconf_permission_pid=$!
  zeroconf_script_cleanup() {
    local outcome=$?
    kill "$zeroconf_permission_pid" 2>/dev/null || true
    wait "$zeroconf_permission_pid" 2>/dev/null || true
    cat "$RUNNER_TEMP/zeroconf-permission.log"
    if [[ $outcome != 0 ]]; then
      # Window-only dayscript captures cannot show a separate system permission alert.
      mkdir -p build/day/screenshots/macos-appkit/default
      screencapture -x build/day/screenshots/macos-appkit/default/permission-desktop.png || true
      /usr/bin/log show --last 5m --style compact --predicate 'process == "mDNSResponder"' | tail -100 || true
    fi
    return "$outcome"
  }
  trap zeroconf_script_cleanup EXIT
fi
