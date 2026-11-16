# Project notes

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
same in uninstall.cmd). Update swaps files in place (rename-running-exe
trick), preserves `config.ini` + `profile.txt`, relaunches with
`--updated`. Engine stop also stops+deletes the WinDivert service (winws
is killed hard, so its WinDivertClose never runs) - keeps
bin\WinDivert64.sys deletable without a reboot.

Signing tool: `cargo run -p sign-manifest -- generate|sign|verify|check`
(see RELEASING.md). Private keys live in release-keys/ - gitignored,
never commit.
