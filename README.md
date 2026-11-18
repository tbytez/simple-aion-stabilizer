# Simple Aion Stabilizer

A tiny Windows app that helps Aion connect when your internet provider blocks it.

## What it actually does

Your ISP can look inside your traffic (this is called DPI - Deep Packet
Inspection) and cut the connection when it recognizes Aion. This app stops
that by changing **how** your packets look on the wire - split, shuffled,
or padded with throwaway packets - so the ISP's filter can't recognize them,
while the Aion server still understands them perfectly.

Concretely, it runs two well-known open-source tools locally on your PC:

* **winws (zapret)**: a small engine that rewrites your outgoing packets
  on the fly using DPI-evasion tricks (packet splitting, decoy packets,
  fake handshakes).
* **WinDivert**: a legit Windows driver that lets programs edit packets
  in-flight. It only runs while the engine is on; press **Stop** and the
  driver unloads immediately.

## What it does NOT do

* **It does not change your IP address.** Aion still sees your real IP.
* **It is not a VPN.** No tunnel, no middle server, no rerouting, no
  encryption of your traffic. Everything goes straight from your PC to
  Aion's servers like before.
* **It does not touch the game.** No memory reading, no file patching, no
  injection, no cheats. It only rewrites packet shapes at the Windows
  network layer.
* **It does not collect data.** No accounts, no telemetry, nothing leaves
  your PC except your normal traffic.

## Use it

1. Download the zip from [Releases](../../releases), extract it.
2. Run `AionStabilizer.exe` (admin rights - needed for the driver).
3. Click the big power button, launch Aion, play.
4. Click it again to stop when done.

Minimizing or closing the window keeps it running in the system tray.
The app follows your Windows dark/light theme automatically.

Handy option: tick **"Start engine automatically when Aion 2 launches"**
and the app watches for the game - it starts the engine when Aion 2 runs
and stops it when you quit. (Watched exe names can be changed with
`watch_process=` in `config.ini`.)

The app checks GitHub for a newer release on start and offers a
one-click in-place update (it keeps your `config.ini` and `profile.txt`).
Disable the check with `update_check=0` in `config.ini`.

## Planned

- **Multi-profile support**: pick a profile in the UI (`profiles/*.txt`)
  so the same app can run zapret strategies for other games without
  editing files.
- **Using it for other games**: today you can already edit `profile.txt`
  filters and `watch_process=` in `config.ini` to target any game or app.
  A short guide will be added.

## Safety notes

- This is the same technique used by AXV Gaming Control Center and the
  large zapret community, tuned down to just Aion.
- No tool can promise zero ban risk. Stop the engine when not playing.
- If it doesn't work for your ISP, edit `profile.txt` and
  `lists\list-aion.txt` (plain text files).
- Not affiliated with NCSoft or AION.

## Build

```sh
rustup target add x86_64-pc-windows-gnu
cargo build --release --target x86_64-pc-windows-gnu
./package.sh <path-to-extracted-zapret-bundle>
```
