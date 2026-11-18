# Project notes

## Devin preferences

For work assigned to Devin, always read and follow the `devin-cli` skill.
Use SWE-2.0 with maximum reasoning instead of the skill's default model.
Resolve the installed provider's exact model identifier before launching;
the verified T3 Devin catalog identifies SWE-2 as `swe-2-high`, with
`thought_level: max`. Do not silently substitute another model or provider.

When the user explicitly requests bypassing permission prompts, apply the
supported bypass mode for that Devin run. For direct CLI runs the skill
documents `--permission-mode dangerous`; T3 uses its supported provider mode.
Bypassing prompts does not expand the task's scope or waive the core-change
approval rules below. Do not interpret this preference alone as a request to
bypass every future run. If Devin is unavailable or out of quota, report the
blocker rather than switching providers without the user's approval.

## Preserve the working core — explicit approval required

The user confirmed v0.1.8 works perfectly on their setup. Treat tag `v0.1.8`
(commit `2a441bacd31dc9d5f53a9a5f1a8eaa6f188af506`) as the working baseline.
Preserve its core behavior unless the user explicitly approves a specific
change in advance.

Protected core includes:

- Network strategy, winws arguments, TCP/UDP handling, target/exclusion lists,
  profile defaults, parsing, validation and migration (`assets/profile*.txt`,
  `assets/lists/`, `src/profile.rs`).
- Backend binaries, driver/pattern versions and pinned hashes
  (`assets/backend-reference.sha256`, bundled `bin/`, packaging inputs).
- Engine start/stop, process detection, automatic startup, controller/tray
  commands, configuration and Windows service handling (`src/engine.rs`,
  `src/controller.rs`, `src/procs.rs`, `src/core.rs`, `src/sys.rs` and callers).
- Native DPI, monitor movement, window sizing and rendering behavior,
  including the vendored winit patch (`vendor/winit/`, `src/rendering.rs`,
  initialization in `src/main.rs`, relevant manifest/build/dependency settings).
- Responsiveness and resize performance: responsive breakpoints, stacked versus
  column layouts, viewport/minimum-size constraints, wrapping and overflow,
  scrolling and scroll-offset recovery, control reachability, scaling behavior,
  resize handling and repaint scheduling (`src/ui.rs` and related code).
- Update verification, installation, recovery and configuration preservation
  (`src/update.rs`, `src/update_install.rs`, `src/manifest.rs`, signing tools,
  `assets/update-recovery.cmd`, `package.sh` and related code).

These boundaries follow behavior, not just filenames. Refactors, dependency
upgrades, optimizations and packaging changes that affect protected behavior
also require approval. A general request to fix, improve, clean up or release
the app does not by itself approve changing the core.

The user explicitly confirmed responsiveness is working perfectly and requires
their approval before any change to it. UI polish, icon work and circle
smoothing must preserve existing responsive behavior and resize performance.
Approval of a visual mockup does not authorize changing responsive rules,
even if the mockup uses different layout thresholds. Before any proposed
responsiveness change, follow the approval process below and identify the
affected window sizes/scales and regression checks. Preserve the existing
layout/input test matrix; do not weaken it to accommodate a visual change.

Before changing protected behavior, investigate without changing it and
present the evidence, exact proposed scope, expected benefit, regression
risks and validation plan. Obtain explicit user approval for that scope before
implementation. If scope expands, obtain approval for the additional changes.
Read-only investigation, isolated tests that preserve production behavior,
documentation and unrelated UI changes may proceed without core approval.

Do not alter the core opportunistically or weaken/remove this approval rule
without the user's explicit approval. Preserve the working baseline and
record validation evidence for any approved change; cross-compilation alone
does not verify Windows behavior or live Aion connectivity.

Windows-only tray app (Rust + native-windows-gui) that wraps zapret's
`winws.exe` with an Aion profile. Built on macOS by cross-compiling.

## Toolchain (one-time)

```sh
brew install rustup mingw-w64
/opt/homebrew/opt/rustup/bin/rustup toolchain install stable
/opt/homebrew/opt/rustup/bin/rustup target add x86_64-pc-windows-gnu
```

IMPORTANT: brew's rustup is keg-only - cargo/rustc shims live in
`/opt/homebrew/opt/rustup/bin`. That dir MUST be in PATH for builds, or
cargo falls back to brew's standalone rustc (no cross targets →
"can't find crate for core").

## Build / verify

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"
cargo check --target x86_64-pc-windows-gnu          # fast typecheck
cargo build --release --target x86_64-pc-windows-gnu
./package.sh <path-to-extracted-zapret-bundle>      # assemble dist zip
```

Cannot run the produced .exe on macOS - verify via `file` (should say
"PE32+ executable (GUI)") and `x86_64-w64-mingw32-objdump -h` (.rsrc section
with manifest + icon).

## Release layout (dist/SimpleAionStabilizer/)

`AionStabilizer.exe` + `profile.txt` + `bin\` (winws/WinDivert from Flowseal
zapret-discord-youtube bundle) + `lists\` (their lists + list-aion.txt +
ipset-all.txt). App resolves everything relative to its own exe path.

In-app updater (src/update.rs): checks GitHub `releases/latest`, requires
both `*-win64.zip` and its sibling `*-manifest.txt` asset (same stem).
The manifest (src/manifest.rs, shared with tools/sign-manifest via
#[path]) is a signed text file: Ed25519 signature over the zip sha256 +
sha256/size of every payload file; public key pinned in
UPDATE_PUBLIC_KEY (placeholder until release keys are provisioned - see
RELEASING.md, updater fails closed). Downloads are byte-capped; the zip
is extracted under entry/size/count limits into a private ACL'd
exclusively-created staging dir under %WINDIR%\Temp that is pinned open
(no FILE_SHARE_DELETE) for the update's lifetime. netsh/sc and other
system tools are invoked by absolute System32 path (sys::system_tool,
same in uninstall.cmd). The swap is delegated to the staged new exe
(`--finish-update <stage_dir>`): it takes the instance mutex (proving
the old app exited), copies all payload files while the old exe is
still in place, swaps the exe last via rename-running-exe trick, then
relaunches with `--updated` - so closing/killing the app mid-update
can't break the install. `update-recovery.cmd` (ships in the zip)
restores the exe from `*.new`/`*.old` after a catastrophic kill.
Preserves `config.ini` + `profile.txt`. Engine stop also stops+deletes
the WinDivert service (winws is killed hard, so its WinDivertClose
never runs) - keeps bin\WinDivert64.sys deletable without a reboot.

## Release naming

GitHub release title AND tag are exactly `v<semver>` (e.g. `v0.1.3`) -
no suffixes, descriptions, or extra text in the title. Detail belongs
in the release notes body.

Signing tool: `cargo run -p sign-manifest -- generate|sign|verify|check`
(see RELEASING.md). Private keys live in release-keys/ - gitignored,
never commit.

## README is user-approved copy

README.md content was written and approved by the user. Do not change it
without explicit user approval.
