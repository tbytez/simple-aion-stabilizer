# Simple Aion Stabilizer

A tiny Windows app that helps Aion connect when your internet provider blocks it.

## What it actually does

The app runs zapret's `winws` engine through the WinDivert network driver.
The default follows the Aion configuration shipped in
[AXV v1.0.0](https://github.com/Alihassanart/AXV-Gaming-Control-Center/releases/tag/v1.0.0):
its host/CDN compatibility rules, QUIC/Discord protocol rules, and destination-
scoped TCP game strategy. Game TCP ports 1024–65535 are modified only for
addresses in `lists/ipset-aion.txt`, initially `193.202.112.0/24` and
`34.117.142.0/24`. Broad high-port UDP game injection is disabled.
This is a system network filter, not a per-process filter; compatibility
rules can also affect matching traffic from other applications.

These are AXV's sample destinations, not an official or exhaustive Aion
server list. If your region uses other addresses, they must be identified
and added to the target list. There is no universal strategy for every
provider, region, or cause of connection failure. A running engine means
the process is alive, not that login succeeded.

Known copies of the old all-destination TCP/UDP profile and the web-only
profile from v0.1.6/v0.1.7 migrate on first start with separate backups.
Custom profiles and existing Aion destination lists are preserved.
The app does not change Windows' global TCP timestamp setting.

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

Requires Windows 10 or newer. The Windows UI draws on the CPU and presents
through GDI, without creating a Direct3D device or DXGI swapchain. The default
window is 420x700 logical points, with a 360x700 minimum capped to fit small
monitor work areas. Title-bar icons refresh at the current monitor scale.

Optional local diagnostics: run with `--cpu-metrics`. For presentation
comparison, add `--cpu-gdi-direct` to use SetDIBitsToDevice instead of BitBlt.
Diagnostics are off by default and never uploaded.

Windows manages window resizing and monitor DPI changes. The interface
wraps and scrolls in smaller windows and uses two columns in wide windows.

1. Download the zip from [Releases](../../releases), extract it.
2. Run `AionStabilizer.exe` (admin rights - needed for the driver).
3. Start the engine **before opening Aion**, then launch the game and test login.
   If the game is already open, close and reopen it for a clean connection test.
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
file. Your existing `lists/ipset-aion.txt` is also preserved. Disable the check with `update_check=0` in `config.ini`.
The update helper verifies its own version and executable against the signed
payload, then verifies the installed executable before relaunch. Failures
show an error dialog and are recorded in `update.log` beside the app.
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
