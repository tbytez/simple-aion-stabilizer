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

Profile tests cover stock-profile migration (including BOM/CRLF), original
backup, repeated loads, custom-profile preservation, conflicting backups,
and the current AXV-reference rules described below. The v0.1.6/v0.1.7
web-only default has been superseded by the v0.1.8 candidate.

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
  matches the current AXV-reference default. From v0.1.6/v0.1.7, check
  profile.pre-aion-tcp.txt instead. Existing backups are retained, with
  numbered names when necessary. Custom profiles stay intact.
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

## v0.1.8 native monitor-boundary fix

The prior layout tests did not execute the Win32 WM_DPICHANGED handler.
This build backports upstream winit PR #4341 to the vendored stable 0.30.13
source. Windows 11 uses the suggested rectangle directly instead of the
legacy monitor-nudging algorithm; known Windows 10 builds keep that algorithm.
The DPI value and font/surface updates still reach eframe normally.

Three additional host tests execute the exact portable policy called by the
patched native handler. They cover modern/unknown Windows builds bypassing
legacy adjustment, Windows 10 retaining it, negative and vertically staggered
rectangles, and 256 back-and-forth 100%/125% cycles with no extra geometry
changes. These are policy tests, not a substitute for native dragging.

Required Windows reproduction uses the reported displays: 2560x1440 at 100%
and 2560x1600 at 125%. Drag by hand (a programmatic move does not exercise the
same native move loop), in both directions, grabbing the left, center and
right of the title bar. Pause with the window straddling the boundary; it
must stop resizing and flickering while the pointer is stationary. Repeat
from a maximized window, after manual resizing, and with monitors arranged
above/below or at negative desktop coordinates. Check that returning to the
same monitor preserves the logical client size apart from pixel rounding,
and that text and hit targets remain sharp and aligned. Also test changing
Display Settings scaling while the app is open and Win+Shift+Arrow transfers.

The native tests require a Windows machine with multiple monitors; they
cannot run in the macOS cross-compilation environment.

## v0.1.8 AXV-reference network profile

The Aion defaults now follow the effective AXV v1.0.0 profile reported working
by the user on WE Egypt: general.bat in TCP game mode, two included IP ranges,
the same host/CDN compatibility rules and split parameters, with the inactive
UDP-12 game rule omitted. See AXV-INVESTIGATION.md for source paths, hashes,
binary comparisons and the explicit remaining differences.

Profile tests verify migration of v0.1.6/v0.1.7's web-only stock file, backup
preservation, target-list validation, the actual engine argument-building path,
IP scope for unknown-protocol TCP, absence of unknown-protocol UDP injection,
and preservation of edited targets during updates.

For runtime validation, fully quit AXV and other winws instances before
starting this app. Start the engine before opening Aion. Repeat fresh logins
and character entry on the same region/WE connection that works with AXV's
unchanged default, and compare engine.log arguments with the inspected profile.
These tests cannot run on the macOS build machine. The native monitor-boundary
fix remains included and still requires a real mixed-DPI hand-drag test.

## Additional pre-release audit fixes

- Re-read update state before the game watcher considers restarting the engine.
  ApplyUpdate must not restart winws in the same command-loop iteration.
- Publish profile changes atomically, retain conflicting prior backups under
  numbered names, tolerate stale temporary files, and support UTF-8 BOM files.
- Initialize RtlGetVersion's input structure size before using its build number
  to select the legacy versus Windows 11 DPI rectangle handling.
- Recovery prefers the previous executable when both .old and .new exist; a .new
  file may have been moved aside after failing post-install verification.
- Package validation rejects missing/empty dependencies and unexpected backend
  binaries. Cargo.lock and the package target directory are fixed explicitly.

The game watcher is detection-based, so it can start after a connection already
began. For AXV-equivalent login validation, start the engine before Aion or use
Start engine on app open. Test watcher timing separately rather than assuming
it is equivalent to pre-launch startup.

## v0.1.9 UI polish

The user confirmed v0.1.8 works on their setup. v0.1.9 changes local UI
drawing only: antialiased gradient rings, a centered header brand mark,
consistent vector icons and hover/focus feedback. The network strategy,
backend assets, engine/controller, updater, native rendering configuration,
winit patch and packaging logic remain identical to v0.1.8. The only main.rs
change declares the new drawing module. Existing responsive thresholds,
scroll behavior and UI test assertions are preserved; the header retains
its wrapping layout rather than forcing the mockup's version alignment.

Independent validation passed 49 app tests and eight signing-tool tests,
including the existing 2,432-layout matrix and input/scroll/focus tests.
Three new drawing tests exercise ring coverage fringes and bounded geometry.
Strict Clippy passes for host and Windows test targets. macOS screenshots
provide visual evidence for the local drawing, not native Windows behavior.

On Windows, inspect both ring edges, icon alignment and keyboard focus at
100%, 125%, 150%, 200% and higher supported scaling in both themes. Repeat
the already-working resize and mixed-monitor interactions to check for
regressions. Native Windows visual/performance checks have not been run in
the macOS environment. Tray and executable icon assets are unchanged.

## v0.1.10 single-column polish and identity assets

This release applies the approved compact visual spacing and uses the same
vector A mark for the header, window/executable icon and state-colored tray
icons. The default remains 420x700 logical points. Existing responsive
thresholds, scrolling, focus handling and all existing UI test assertions
are unchanged. Network profiles, engine/controller, updater, native DPI and
renderer selection remain identical to v0.1.9.

Independent tests pass 49 app tests and eight signing-tool tests, including
the complete layout/scale and input matrix. Strict Clippy passes on host and
Windows targets. Native icon sizes and repeatable generation are verified.
Actual macOS Rust captures cover dark, light and running states; Windows
icon-cache, tray legibility and physical monitor/resize checks remain pending.

NVIDIA adapter-preference changes belong to a separate, unpublished test
build. They are not included in this release. Neither CPU rendering nor
cross-compilation establishes that the NVIDIA FPS overlay is prevented.

## v0.1.11 settings alignment and footer fit

Settings icons and switches now center on their complete text blocks,
including wrapped subtitles. Compact chip/status/footer rows remove accidental
minimum row height so the footer and update button fit the default 420x700
logical-point window. Font sizes, control targets, responsive thresholds,
scroll/focus handling, native DPI, renderer and engine behavior are unchanged.

Validation: 52 app tests and eight signing-tool tests pass, with three new
regressions covering painted icon/switch alignment, wrapped subtitles and
default-window footer/button visibility across themes, states and scales.
Strict Clippy passes on host and Windows targets. Actual macOS captures show
the default window in dark, light and running states. Native Windows visual,
resize, mixed-DPI and update/relaunch checks remain unverified on this host.

The unpublished WARP-preference experiment suppressed the overlay on the
user's Windows setup but caused severe resize lag. It is excluded; this
release retains the stable hardware-preferred renderer.
