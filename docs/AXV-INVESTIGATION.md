# AXV Aion configuration comparison (2026-11-16)

Inspected [AXV v1.0.0](https://github.com/Alihassanart/AXV-Gaming-Control-Center/releases/tag/v1.0.0)
without running its programs. The repository contains a README; the release ZIP
also contains Python source, backend scripts, saved settings and an initial
working backup. ZIP SHA-256:
`771f5b7238d7d181f56aeec4209bdb3b41ada370e2b6a486f1fde42388f428f5`.

## What the supplied configuration actually does

`data/profiles.json` selects `general.bat`, game mode `tcp`, TCP ports
1024–65535, and two included ranges: `193.202.112.0/24` and `34.117.142.0/24`.
`data/settings.json` selects standalone process mode. `source/axv/strategies.py`
substitutes the selected profile's IP file into every `--ipset` argument and
sets the disabled game-UDP range to port 12. Thus the saved game setting does
not enable broad UDP 1024–65535 manipulation.

`backend/general.bat` uses multisplit with overlap 568, split position 1,
and `tls_clienthello_4pda_to.bin` for the included destinations' TCP rules.
Its high-port TCP rule also permits unknown protocols, with cutoff `n3`.
The general launcher still contains QUIC (UDP 443), Discord and unrelated
host-list rules even though the selected game mode is TCP. The scoped UDP
443 rule uses six fake QUIC packets; this is different from the disabled
high-port UDP rule that injects unknown game payloads twelve times.

`source/lite.py` launches winws directly with an argument array. It checks for
competing engines and waits 400 ms to catch startup failures. The standalone
start path does not invoke BAT setup or change global TCP timestamps. It is
manual, so the engine can be running before the game opens.

## Binary comparison with our existing bundle

All five files below are byte-identical to the Flowseal 1.10.4 files already
bundled by this app; no replacement executable from AXV was needed:

| File | SHA-256 |
| --- | --- |
| winws.exe | affb4f69d2ea302a7abccd5325d81826e140ddae014f1e070bc4a6c0dd555188 |
| tls_clienthello_4pda_to.bin | eefeaf09dde8d69b1f176212541f63c68b314a33a335eced99a8a29f17254da8 |
| quic_initial_www_google_com.bin | f4589c57749f956bb30538197a521d7005f8b0a8723b4707e72405e51ddac50a |
| WinDivert.dll | c1e060ee19444a259b2162f8af0f3fe8c4428a1c6f694dce20de194ac8d7d9a2 |
| WinDivert64.sys | 8da085332782708d8767bcace5327a6ec7283c17cfb85e40b03cd2323a90ddc2 |

## Changes in this app

- Match the reference's effective default rules, including host-based CDN
  compatibility, Google/Discord host rules and QUIC/Discord protocol rules.
  These may cover login dependencies outside the two game destination ranges.
  The user confirmed the unchanged AXV default works reliably on the same WE
  connection; retaining the full effective rules gives a concrete baseline.
- Keep high-port game TCP destination-scoped with the reference split/overlap
  parameters. Omit the inactive game-UDP port-12 rule and placeholder-only
  user-list entries. Do not enable arbitrary high-port UDP game injection.
- The host and Google lists are byte-identical in our existing bundle. Supply
  the inspected reference's exclusions separately as list-compat-exclude.txt
  so a different upstream exclusion-list revision does not change the baseline.
- Use lists/ipset-aion.txt for the two game ranges, not the old all-IPv4 list.
  Reject empty/malformed/all-IPv4 target lists before starting the engine.
- Migrate known broad and web-only stock profiles with separate backups.
  Preserve custom profiles and edited destination lists across updates.
- Record actual engine arguments in engine.log and wait 400 ms on the
  controller thread to detect immediate startup failure.
- Keep Stop as a direct-connection fallback. For a clean test, start the
  engine before opening Aion, rather than relying on a later process poll.

This is a system-wide network filter while active. It is not a per-process
filter and can affect other traffic matching the reference compatibility rules.
No AXV Python application code is incorporated; this implements the inspected
Flowseal command/configuration values using our existing engine lifecycle.

This is a configuration-based starting point, not proof that AXV or this app
fixes every Aion region/provider. The ranges are AXV's supplied values, not a
verified complete/current Aion endpoint inventory. No live Aion connection
or WE Egypt route is available in the build environment. If retries remain,
compare the actual remote endpoints with the target list before changing
ports or protocols again. The native monitor-boundary fix is independent.
