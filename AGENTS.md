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

In-app updater (src/update.rs): checks GitHub `releases/latest`, expects a
release asset named `*-win64.zip` containing the folder above. Update swaps
files in place (rename-running-exe trick), preserves `config.ini` +
`profile.txt`, relaunches with `--updated`.
