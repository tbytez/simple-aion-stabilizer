# v0.1.8 candidate audit — 2026-11-17

Status: automated checks passed. The user subsequently requested publication
of v0.1.8 with the Windows runtime checks below still unverified. Publication
does not change their status; the release notes must retain this limitation.

## Findings resolved

| Finding | Correction and evidence |
| --- | --- |
| Applying an update could stop winws and then restart it in the same controller iteration because the game watcher used an earlier update-state snapshot. | The watcher re-reads update state after command handling. A regression test covers the state transition. This prevents this path from relocking engine files during installation. |
| Stock-profile migration could be blocked by an existing, different backup; direct writes could leave a partial profile. | Exclusive temporary files, sync and rename publish the profile atomically. Conflicting backups receive numbered names; stale temporary files do not block migration. Tests cover these paths and failure preservation. |
| BOM-prefixed profiles and target lists could fail parsing. | Both parsers accept UTF-8 BOM and CRLF; tests exercise the engine argument-building path. |
| The Windows build-number query did not initialize its required structure-size field. | Set the size before RtlGetVersion, so the native DPI policy can select the known Windows 10 behavior correctly. |
| Emergency recovery preferred a .new executable even when it might have failed verification. | Prefer .old when the final executable is missing, and report rename failures. Native recovery remains a runtime gate. |
| Packaging could accept missing profile dependencies or silently use a different backend; an external Cargo target directory could leave a stale executable at the expected path. | Validate dependencies and eight pinned backend hashes; enforce Cargo.lock and the package target directory. Negative packaging checks reject invalid payloads. |

The monitor-boundary change backports upstream winit PR
[#4341](https://github.com/rust-windowing/winit/pull/4341) to stable 0.30.13.
The legacy monitor-nudging logic is retained for known Windows 10 builds;
Windows 11 and unknown builds use the WM_DPICHANGED suggested rectangle.
Only the documented Windows implementation files differ from the original
crate, alongside the added policy module and patch notes. Policy tests do not
prove the real native drag loop is fixed on the user's hardware.

## AXV comparison

The user confirmed AXV's unchanged default works consistently on the same
WE Egypt connection. The comparison used the source and bundled assets in
AXV v1.0.0's release archive, including its selected profile and argument
substitution logic; it did not infer the strategy from the README alone.

The candidate contains 67 arguments across eight active rules matching the
normalized effective AXV general.bat configuration in TCP game mode. This
includes host/CDN, QUIC and Discord compatibility rules plus game TCP scoped
to the two selected IP ranges. The inactive UDP-port-12 game rule and
placeholder user-list arguments are omitted. Unknown-protocol game UDP and
the all-address IP set are not enabled.

All eight pinned engine/driver/pattern files match the reference byte for
byte. They also match the v0.1.7 package, avoiding unnecessary replacement
of those unchanged backend files during upgrade. Shared stock host lists
match the reference. See [AXV-INVESTIGATION.md](AXV-INVESTIGATION.md) for
provenance, hashes and remaining differences.

Starting the engine after game detection can miss the beginning of a
connection. Start it before opening Aion for the AXV comparison. A running
winws process establishes startup success, not login or gameplay success.

## Completed verification

- 54 host tests pass: 46 app tests and eight signing-tool tests.
- Strict Clippy with warnings denied passes on the host and Windows test
  target. Windows-only tests compile; they were not executed here.
- UI tests cover 2,432 layout combinations, input/scroll/focus behavior,
  fractional DPI and very small viewports. They do not measure Windows
  frame timing or perceived text sharpness.
- Three native-DPI policy tests cover the version split and repeated
  100%/125% rectangle transitions, including negative/staggered coordinates.
- Actual historical stock profiles from v0.1.3–v0.1.5 match the legacy
  migration input; v0.1.6/v0.1.7 match the web-only migration input.
  Custom profiles and edited target lists are preserved.
- Portable installer tests cover exact executable replacement, failed
  promotion, rollback, payload-copy failure, and preserved config/profile.
  A stock upgrade followed by first-start migration needs one installation.
- Packaging checks accept paths containing spaces and BOM/CRLF profiles;
  reject missing dependencies, empty dependencies, modified backend files,
  invalid tokens and comments-only profiles.
- The final ZIP was freshly extracted. Its Ed25519 manifest signature and
  all 48 payload files pass verification; its ZIP SHA-256 matches the
  signed manifest. The public key matches the app's pinned key.
- The packaged executable equals the current release build, is an x64 PE
  GUI executable, and contains the expected manifest/icon resource section
  and v0.1.8.0 version metadata. Packaged profile, recovery script and docs
  match the current sources.
- Shell syntax checks and git diff whitespace checks pass.

## Required Windows release gates

These have **not** passed in the macOS build environment. Record evidence
for each check before treating its corresponding behavior as verified.

1. **Mixed-DPI movement and resize:** use the reported 2560×1440 at 100%
   and 2560×1600 at 125%. Drag by hand in both directions using different
   title-bar grab positions; pause across the seam. Check for flicker,
   repeated resizing, blurry text and misaligned clicks. Repeat after
   maximize/restore and manual resize. Check higher supported scaling,
   negative/staggered monitor coordinates, and scaling changes while open.
2. **WE/EU login:** quit AXV and competing winws processes. Start this
   engine before Aion, then repeat fresh logins and character entry on the
   same WE connection. Record the attempt count and failures, and verify
   gameplay. Compare against AXV's unchanged default if any failures remain.
   Verify game-detection timing separately.
3. **One-step update and recovery:** update an actual v0.1.7 installation
   once with the game watcher active. The automatic restart must show
   v0.1.8, and another update check must report it is current. Verify
   update.log, configuration preservation, no engine restart during the
   handoff, and interrupted-update recovery. Portable file-operation tests
   do not execute the Windows process/mutex/relaunch path.

The detailed interaction matrix is in
[WINDOWS-VALIDATION.md](WINDOWS-VALIDATION.md). Record OS build, monitor
arrangement/scaling, candidate hash and results with the runtime evidence.

## Audited artifacts

Release artifacts: `dist/SimpleAionStabilizer-v0.1.8-win64.zip`, with sibling
`SimpleAionStabilizer-v0.1.8-manifest.txt`.
This audit report is outside the signed payload to avoid a circular ZIP hash.

```text
ZIP SHA-256:
b394362ce39b9aa38e775fa3e09b080c13a5f0a3b119170d5e397a6fe91e3900
Executable SHA-256:
658d70ff259aeeb1e6cd17f45d92c115471fa1cabba50d48c6073913fdb7cae1
Normalized profile arguments SHA-256:
0dccda44ebe71b04d1ab6eb800e316a33e61743fd48b204758b8d8b0af6c1ea7
```
