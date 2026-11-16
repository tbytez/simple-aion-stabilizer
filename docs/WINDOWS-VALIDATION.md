# Windows validation

Automated UI verification covers 19 scale factors: 100%, 110%, 125%, 137%,
150%, 175%, 187.5%, 200%, 225%, 250%, 262.5%, 300%, 350%, 400%, 450%,
475%, 500%, and synthetic 600%/800% stress cases. These include standard
Windows settings and fractional values to exercise rounding.

The layout matrix checks both themes and four states (stopped, running,
long engine error, and long update error) at 16 viewport sizes per scale:
small, tall/narrow, wide/short, normal, sizes around the column breakpoint,
2048x1280 logical points, and physical displays of 1280x720, 2560x1600,
3840x2160, 5120x2880 and 7680x4320 divided by each native scale. This gives
2,432 layout combinations. Text must fit horizontally in normal-width
windows. Below 300 logical points, both axes scroll deliberately, allowing
windows to fit displays that are too small for the normal layout width.

Interaction tests exercise actual pointer/key/wheel events and verify
controller commands, covering:

- Start and Stop after repeated mixed-DPI and wide/narrow transitions.
- Scrolling to settings, update checks and long errors at every scale.
- Error dismissal and scroll-offset recovery when content shrinks.
- Both-axis scrolling and clicks at 160x90 logical points (720p at 800%).
- Widening after horizontal scrolling, without retaining a stale X offset.
- Disabled Start while the updater is downloading.
- Zero-sized viewport followed by restore to a normal window.
- Tab, Enter and Space with automatic scrolling to a newly focused control.

These tests verify logical layout, font tessellation, and input handling.
They do not establish perceived text sharpness, frame timing, Win32 window
placement, or performance on Windows hardware.

Profile tests cover stock-profile migration (including CRLF), original
backup, repeated loads, custom-profile preservation, conflicting backups,
and the absence of UDP, unknown-protocol, global-IP-set, fake-packet and
sequence-overlap rules in the new default.

## Runtime checks still required on Windows

- On 720p/1080p, 2560x1600, 4K, 5K and 8K displays where available,
  test native scales from 100% through 500% and supported custom scaling.
  Inspect text sharpness, resize continuously in both directions, and verify
  every control remains reachable. Very small logical viewports should
  provide horizontal and vertical scrolling instead of forcing an oversized
  minimum window. The 600%/800% headless cases are stress tests, not a claim
  that Windows offers those values in Display Settings.
- Move between monitors at different scales in both directions repeatedly.
  Check text sharpness, click targets, and resize responsiveness.
- Maximize and restore. Minimize/close to tray, reopen, and repeat.
- Start/stop using the main button and tray. Stop while the game is opening:
  auto-start must stay off until the game is observed to have exited.
- On an installation upgraded from v0.1.5, start once and check that
  profile.pre-domain-scope.txt contains the old profile and profile.txt
  now matches the new domain-scoped default. Custom profiles stay intact.
- On the reported EU / WE Egypt connection, compare a fresh login with
  engine stopped versus started. Stop between attempts and reconnect so
  the engine sees the beginning of each connection. Test character entry
  and actual gameplay as well as the login screen.
- If startup fails, inspect engine.log for driver or argument errors. If
  the engine stays alive but login fails, capture the failing hostname or
  destination and the log before selecting an ISP-specific strategy.
  Do not restore blanket all-port UDP manipulation as an assumed fix.

No Windows runtime or live Aion/WE connection is available in the macOS
build environment. Cross-compilation and headless layout tests do not
establish that the above runtime checks passed.

## v0.1.7 updater and restored design

The power ring, status chips, switch rows and compact typography are restored
inside layout-managed cards. The ring glow is static, and the native window
frame still handles resizing. Rendered dark/running and light/stopped previews
were inspected on macOS. The existing scale matrix and input tests still pass.

Six portable installer tests verify a single successful handoff installs the
exact new executable, preserves config/profile, rejects an old helper's version
or bytes, and rolls back failed promotion or post-rename corruption. Payload
copy failure must leave the old executable in place.

Three Windows-only tests cover acquiring an existing unowned mutex, retrying
after contention, and exclusive stage claims/live-owner markers. These compile
for the Windows target but have not been executed in the macOS environment.

On Windows, update once from v0.1.5/v0.1.6 to v0.1.7. Confirm the header reports
v0.1.7 immediately after the automatic restart, a subsequent update check
reports the latest version, and update.log records executable verification.
Also test starting a second instance during the handoff, a temporarily locked
backup executable, and an interrupted helper. Startup cleanup must skip live
update payloads. A genuine install failure must show an error and log the
reason rather than silently presenting the old version as an updated app.
