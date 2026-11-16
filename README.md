# Simple Aion Stabilizer

A tiny Windows app that helps Aion connect when your internet provider blocks it.

## What it actually does

The app runs zapret's `winws` engine through the WinDivert network driver.
The default profile splits HTTP/TLS packets only for domains in
`lists/list-aion.txt`. It leaves UDP and unknown game protocols untouched.
It does not improve every connection: the correct strategy depends on the
provider, region, and cause of the connection failure. A running engine
only means the process is alive, not that Aion login succeeded.

The earlier bundled profile intercepted nearly all high TCP/UDP ports and
injected fake packets into unknown protocols. That could disrupt login or
other applications. On first engine start, known copies of that old profile
are backed up to `profile.pre-domain-scope.txt` and replaced with the scoped
profile. Custom profiles are preserved; review them if login still fails.
The app no longer changes Windows' global TCP timestamp setting.

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

Requires Windows 10 or newer with DirectX 12 support. The UI prefers an
integrated GPU, then other compatible hardware, with Microsoft WARP as a
software fallback. It does not use OpenGL. To force the software fallback
for troubleshooting, set `AION_SOFTWARE_RENDERING=1` before starting the app.

Windows manages window resizing and monitor DPI changes. The interface
wraps and scrolls in smaller windows and uses two columns in wide windows.

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
Updates install only after a signed manifest verifies every payload
file. Disable the check with `update_check=0` in `config.ini`.
If an update is ever interrupted so hard that `AionStabilizer.exe`
disappears, run `update-recovery.cmd` in the app folder to restore it.

To remove the app: quit it from the tray (stopping the engine also
unloads the WinDivert driver), then delete the folder - no reboot needed.
If files are still locked, run `uninstall.cmd` as administrator first.

## Antivirus reports

Avast may send an unfamiliar executable to its Threat Labs through
[CyberCapture](https://support.avast.com/avast/en/ww/home/current/solutions/antivirus-cybercapture).
"Sent for analysis" is a pending review, not a final malware verdict.
Wait for Avast's result. New builds may undergo analysis again; no release
can guarantee skipping that process. Windows version metadata and signed
update manifests identify the release, but the manifest signature is not
an Authenticode publisher certificate or antivirus clearance.

## Planned

- **Multi-profile support**: pick a profile in the UI (`profiles/*.txt`)
  so the same app can run zapret strategies for other games without
  editing files.
- **Using it for other games**: today you can already edit `profile.txt`
  filters and `watch_process=` in `config.ini` to target any game or app.
  A short guide will be added.

## Safety notes

- If login fails, stop the engine and retry. Stop holds even with game
  auto-start enabled until the watcher observes that the game has exited.
- Engine startup and driver messages are saved in `engine.log` beside the
  app (replaced each start). Include the log, region, provider, and whether
  login works with the engine stopped when reporting a connection issue.
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

`package.sh` also signs the update manifest when a signing key exists
(`release-keys/update-private.key` or `AION_UPDATE_KEY`). See
[RELEASING.md](RELEASING.md) for key provisioning and release gates -
unsigned releases are refused by the updater.
