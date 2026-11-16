# Simple Aion Stabilizer

**A tiny Windows app that helps Aion connect when your internet provider gets in the way.**

Some ISPs block, slow down, or break the connection to Aion's servers.
This app fixes that for many people — without a VPN, without installing
anything, and without touching the game itself.

## What it does (simple version)

* You press **Start** → the app quietly fixes how your PC's network traffic
  looks so your ISP's filters can't mess with the Aion connection.
* You press **Stop** → everything goes back to normal instantly. Nothing
  stays running in the background.
* It never touches the game, your account, or your files. It only changes
  how *your own computer* sends packets — similar in spirit to a VPN, but
  nothing is routed through another server.

That's it. Two buttons, one status light, and it lives in your system tray
(next to the clock) so it's out of the way.

## How to use it

1. Download the latest `SimpleAionStabilizer-vX.Y.Z-win64.zip` from
   [Releases](../../releases) and extract it (for example to
   `C:\Apps\SimpleAionStabilizer`).
2. Double-click `AionStabilizer.exe`. Windows will ask for
   **administrator permission** — that's needed for the network driver.
3. Press **Start**.
4. Launch Aion and play.
5. When you're done, press **Stop** (from the window or the tray icon).

Tips:

* **Closing the window doesn't quit the app** — it hides in the tray.
  Left-click the tray icon to bring it back, right-click for the menu,
  choose **Exit** to fully quit.
* Turn on **Run at Windows startup** to have it ready every time you boot.
* Turn on **Start engine when app opens** to skip pressing Start at all.
* Keep the whole folder together — the exe needs `bin\`, `lists\` and
  `profile.txt` next to it.

## If it doesn't work for you

Every ISP is different. The app comes with a broad default profile
(based on zapret "general ALT6" + game filter) that works for most people.
If yours doesn't connect, or it breaks other sites:

* `profile.txt` — the exact engine settings, editable with Notepad.
* `lists\list-aion.txt` — Aion/NCSoft domains; add your region's servers.
* `lists\ipset-all.txt` — `0.0.0.0/0` means "apply to everything";
  replace it with your game server's IP ranges to narrow it down.

Lots of alternative profiles are documented by the zapret community:
[bol-van/zapret](https://github.com/bol-van/zapret) and
[Flowseal/zapret-discord-youtube](https://github.com/Flowseal/zapret-discord-youtube).

## Is it safe? Will I get banned?

Honest answer: **no third-party tool can promise zero risk** — but this one
is designed to be as safe as it gets:

* It does **not** read or write the game's memory, files, or data. It is not
  a cheat and gives no gameplay advantage.
* It only adjusts network traffic at the Windows level — the same approach
  used by AXV Gaming Control Center and the large zapret community.
* Pressing **Stop** unloads everything immediately. If the game or its
  anti-cheat ever complains, stop the engine and you're back to stock.

Use it at your own risk, and stop the engine when you're not playing.

## For developers

Small Rust app (~0.4 MB exe, no runtime) using
[native-windows-gui](https://github.com/gabdube/native-windows-gui) for the
interface. It launches `winws.exe` (zapret) with an Aion profile and watches
the process. Minimize/close hides to tray; the tray menu mirrors the window
controls.

Build (needs rustup + `x86_64-pc-windows-gnu` target + MinGW-w64 on PATH):

```sh
rustup target add x86_64-pc-windows-gnu
cargo build --release --target x86_64-pc-windows-gnu
./package.sh <path-to-extracted-zapret-bundle>
```

`package.sh` builds the exe and assembles the release zip in `dist/`.

## Disclaimer

Independent community project — **not affiliated with NCSoft, AION, AION 2,
the zapret authors, or Flowseal**. Some network-filtering techniques may
conflict with anti-cheat software, ISP rules, or terms of service.
Third-party binaries in the release (winws, WinDivert) remain under their
own licenses — see `THIRD_PARTY_NOTICES.txt`.
