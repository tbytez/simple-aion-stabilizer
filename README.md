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
3. Press **Start**, launch Aion, play.
4. Press **Stop** when done.

Minimizing or closing the window keeps it running in the system tray.

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
