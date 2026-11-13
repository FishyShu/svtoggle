# Changelog

## 0.3.1 — 2026-11-13
- New mascot logo: a chubby amber fish swimming past the globe, drawn in code (`app/src/icon.rs`), used for the exe, window, tray, favicon and website. Simplified automatically at 16-20 px.

## 0.3.0 — 2026-11-13
- Protected ranges are now personal config: `%APPDATA%\svtoggle\protect.txt` (one CIDR per line). The repo ships none. Ranges listed there are stripped from every region on load and refresh, and the block-time guard still refuses anything touching them.
- MIT license, public repo.

## 0.2.2 — 2026-11-13
- Portable mode: an exe whose name contains "portable" (the release ships `svtoggle-portable.exe`) keeps presets, cache and settings in a `svtoggle-data` folder next to itself instead of `%APPDATA%`. Same single file, nothing to install; it still asks for admin since firewall rules need it.

## 0.2.1 — 2026-11-13
Fixes from a Codex review:
- Installer: the no-UAC task now runs a copy in `Program Files` (it was in user-writable `%LOCALAPPDATA%`, a silent-admin hole). Fixed the `-DontStopIfGoingOnBatteries` typo that stopped task registration; the elevated window now stays open on errors.
- Overwatch: one range belongs to one region. Community lists had Singapore's /16 covering 8 GCP regions, Dammam (ME) inside "EU", Taiwan inside Japan. Broader overlapping ranges are dropped, exact duplicates go to the true owner. Applied on load and on refresh.
- Firewall rules are disabled while being updated and only re-enabled once scoped to the game exe.
- Allow everything disables the whole svtoggle rule group, so rules for cities removed by a refresh can't linger; refresh also turns those off.
- Refresh no longer hides a re-block error behind its success message.
- Ping rounds can't pile up threads; match detection fails closed if UDP port ownership can't be read; single-instance startup race fixed; COM initialised once per thread.

## 0.2.0 — 2026-11-13
- New look: graphite theme, IBM Plex fonts, animated switches instead of Block/Allow buttons, one amber accent that only means "in a match".
- Real app: embedded icon, admin manifest (double-click asks for UAC itself), version info, window icon.
- World map with every region/city; click a dot to toggle.
- Deadlock: one toggle per Valve city, grouped by area with area switches. Valve's broken Datapacket coordinates fixed.
- Presets (save / apply / right-click delete), Allow everything, Fastest now.
- Refresh lists from Valve / community lists; cache stores only IP lists so app updates always win.
- Match detection banner, tray icon, single instance, remembers the last tab.
- Overwatch Middle East ping now goes to AWS UAE (AWS Bahrain refuses connections from some networks).

## 0.1.0 — 2026-11-13
- First version: Overwatch regions + Deadlock region groups, firewall rules scoped to the game exe, protected-ranges guard, CLI.
