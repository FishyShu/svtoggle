<p align="center"><img src="docs/logo-256.png" width="96" alt="svtoggle logo"></p>

<h1 align="center">Deadlock &amp; Overwatch Server Selector</h1>

<p align="center">
  Your ping deserves better than a server three countries away.<br>
  <b>svtoggle</b> picks which server regions you can be matched on, for <b>Deadlock</b> and <b>Overwatch</b> on Windows.<br>
  World map, per-city toggles, ping per region, presets. Firewall rules only: no injection, game files untouched.
</p>

<p align="center">
  <a href="https://github.com/FishyShu/deadlock-overwatch-server-selector/releases/latest"><b>Download</b></a> ·
  <a href="https://fishyshu.github.io/deadlock-overwatch-server-selector/">Website</a> ·
  <a href="#faq">FAQ</a>
</p>

> **Beta, honestly.** svtoggle blocks the server addresses it *knows about*. It can't force the game to pick a region, and Deadlock blocking and match detection are still experimental. Better you read it here than find out mid-queue.

![svtoggle Deadlock tab: world map of Valve server cities with ping and region switches](docs/screenshot-deadlock.png)

Looking for a **Deadlock region changer** (the old `citadel_region_override` console command no longer exists),
an **Overwatch server selector**, or a way to **block Middle East / Asia / NA servers**? You found it. Hi.

## Install
**Recommended:** download `svtoggle-<version>-windows.zip` from the latest release, unzip it, and run
`powershell -File install.ps1` (it asks for UAC once). It copies the app to `%ProgramFiles%\svtoggle` (admin-only folder, since the task runs it elevated),
registers a `svtoggle` scheduled task and makes Desktop and Start Menu shortcuts that launch through it, so later launches show no UAC prompt.

**Quick try:** double-click `svtoggle-gui.exe` from the ZIP (or build it, see below). It asks for admin on every launch, because firewall rules need it.

**Portable:** use `svtoggle-portable.exe` instead (or rename any copy to contain "portable"). Same file, but it keeps presets and cached lists in a `svtoggle-data` folder beside itself, so it can live on a USB stick.

Overwatch is detected from Battle.net or Steam installs; if the game isn't found, use the **Locate game** button to point svtoggle at the game exe.

## Using it
- **Switch on** = that region is reachable. **Switch off** = blocked. Click a dot on the map to do the same.
- Deadlock is grouped by area; the area switch flips every city in it, the chevron opens the cities.
- **Fastest now** lists the three lowest pings you can currently reach.
- **Presets**: block what you want, *Save current*, name it. One click (or the tray menu) brings it back. Right-click a preset to delete it.
- **Allow everything** turns off every svtoggle rule for both games.
- **Refresh lists** downloads the newest server lists and re-applies whatever you have blocked.
- While a game is running, the right panel tries to detect which region your match is on (experimental).
- Blocking in the middle of a match asks first. A dropped comp game is a leaver penalty, and the fish refuses to be blamed for that.
- Closing the window keeps it in the tray. Right-click the tray icon to quit (your blocks stay on, it'll remind you).

## How it works
- One outbound Block rule per region (`svtoggle <game> <region>`, group `svtoggle`), **scoped to the game's exe**, so other apps never lose those cloud ranges.
- Allow **disables** a rule, never deletes it.
- **Protected ranges**: list ranges you never want blocked (e.g. your nearest home region) in `%APPDATA%\svtoggle\protect.txt`, one CIDR per line. They're stripped from every region on load and any block touching them is refused.
- Overwatch ranges: a hand-verified Middle East list + [Qiiks/ow2-server-list](https://github.com/Qiiks/ow2-server-list) for the rest (community data, protected ranges stripped).
- Deadlock ranges: exact Valve relay IPs per city from `ISteamApps/GetSDRConfig` (appid 1422450). Cities that publish no relays (`dsto`, `eat`, `fsn`, `hel`) can't be blocked. Valve's coordinates for the Datapacket cities are wrong, so they borrow their twin city's.
- Ping: Overwatch = estimated (AWS proxy: the nearest AWS region, not the game server). Deadlock = ICMP to a real relay.
- Match detection: counts UDP packets on the game's own ports with a raw socket (admin only).

## CLI
```
svtoggle overwatch status
svtoggle deadlock block ams
svtoggle deadlock refresh
svtoggle monitor          # what match detection sees, once a second
```

## Undo everything
- Click **Allow everything** in the app to disable all rules without uninstalling.
- Full uninstall: `powershell -File install.ps1 -Uninstall` (or `scripts\install.ps1`). It stops a running svtoggle, removes **all** firewall rules in group `svtoggle`, the scheduled task, both shortcuts and the `%ProgramFiles%\svtoggle` folder, then verifies each removal and exits non-zero if anything is left.
- **Kept:** your settings, presets and cached lists in `%APPDATA%\svtoggle` (delete with `Remove-Item -Recurse "$env:APPDATA\svtoggle"`; portable mode uses `svtoggle-data` beside the exe).
- **Untouched:** your own firewall rules outside the `svtoggle` group (e.g. old hand-made `OW2 Block ...` rules).
- Rules only, admin PowerShell: `Get-NetFirewallRule -Group svtoggle | Remove-NetFirewallRule`

## Build
`cargo build --release` → `target/release/svtoggle-gui.exe` (icon + admin manifest embedded, needs the Windows SDK) and `svtoggle.exe`. `cargo test` runs the guard tests.

## FAQ
**How do I change my server region in Deadlock?** `citadel_region_override` is gone from current builds. svtoggle blocks the Valve relay cities you don't want, so the game can't connect to the ones you block (whether it then picks another region or fails is up to the game).

**How do I block Overwatch Middle East servers?** Overwatch tab, switch off *Middle East*. The rule only applies to `Overwatch.exe`.

**Can this get me banned?** It never touches the game; it only adds Windows Firewall rules, like a router geo-filter. Use at your own risk, and don't switch regions mid competitive match: dropping out can count as leaving.

**Stuck on "connecting"?** You probably blocked everywhere it wanted to go. Click *Allow everything*, then block a little less greedily.

**Linux or Mac?** Windows 10/11 only. It's built on Windows Firewall, so the penguin and the apple are out of luck.

## Not verified yet (help wanted)
- Deadlock: whether the game fails over to another relay or refuses to connect when a city is blocked.
- Match detection during a real match, and the 20 pkt/s threshold.

Tried one of these? [Open an issue](https://github.com/FishyShu/deadlock-overwatch-server-selector/issues) and tell us what happened, good or cursed.

License: MIT. Fonts, map data and IP-list sources: see [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md). Not affiliated with Valve or Blizzard.
Built by FishyShu, fueled by questionable amounts of energy drinks.
