-- CI-only helper for the English macOS runner. Accept only this demo's local
-- network alert, never unrelated permission or security dialogs. No TCC database edits.
-- The shell wrapper bounds its lifetime to the dayscript run.
on run
    tell application "System Events"
        repeat 180 times
            repeat with processName in {"UserNotificationCenter", "CoreServicesUIAgent"}
                if exists process processName then
                    tell process processName
                        repeat with w in windows
                            try
                                set elements to entire contents of w
                                set promptText to ""
                                repeat with e in elements
                                    if role of e is "AXStaticText" then
                                        set promptText to promptText & " " & (value of e as text)
                                    end if
                                end repeat
                                if promptText contains "local network" and (promptText contains "Zeroconf Explorer" or promptText contains "Demo") then
                                    repeat with e in elements
                                        if role of e is "AXButton" and name of e is "Allow" then
                                            click e
                                            log "Accepted Zeroconf Explorer local-network permission"
                                        end if
                                    end repeat
                                end if
                            end try
                        end repeat
                    end tell
                end if
            end repeat
            delay 1
        end repeat
    end tell
end run
