# Simple Aion Stabilizer

A tiny Windows app that helps Aion connect when your internet provider blocks it.

## Use it

1. Download the zip from [Releases](../../releases), extract it.
2. Run `AionStabilizer.exe` (it needs admin rights).
3. Press **Start**, launch Aion, play.
4. Press **Stop** when done.

That's it. It hides in the system tray when minimized or closed.

## Notes

- It does not touch the game, your account, or your files. It only adjusts
  how your PC sends network traffic, like a VPN but without a server.
- No tool can promise zero ban risk. Stop the engine when not playing.
- If it doesn't work for your ISP, edit `profile.txt` and
  `lists\list-aion.txt` (they are plain text).
- Not affiliated with NCSoft or AION.

## Build

```sh
rustup target add x86_64-pc-windows-gnu
cargo build --release --target x86_64-pc-windows-gnu
./package.sh <path-to-extracted-zapret-bundle>
```
