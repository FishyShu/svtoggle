# Third-party notices

svtoggle itself is MIT licensed (see `LICENSE`). It bundles or uses the following.

## Fonts: IBM Plex Sans and IBM Plex Mono
Copyright © 2017 IBM Corp. with Reserved Font Name "Plex". Licensed under the SIL Open Font License 1.1.
Full text: `OFL.txt` (in the release ZIP; `app/assets/fonts/OFL.txt` in the repo).

## Map data: Natural Earth
Land outlines from [Natural Earth](https://www.naturalearthdata.com/). Public domain.

## Valve Steam Datagram Relay list
Deadlock relay cities and IPs come from the public Steam Web API (`ISteamApps/GetSDRConfig`, appid 1422450),
fetched on refresh and snapshotted in the app. These are network facts, not Valve code or assets.
svtoggle is not affiliated with or endorsed by Valve.

## Overwatch IP ranges
- The Middle East list is hand-verified against AWS and GCP's published IP ranges.
- Other regions are seeded in `profiles/overwatch.toml` (snapshot of 2026-11-13) and refreshed at runtime from [Qiiks/ow2-server-list](https://github.com/Qiiks/ow2-server-list)
  (`ow2-servers.json`, pinned reference: commit `dbfd477ff00ad68b709b2be7df18550ad8ef243d` of `main`, checked 2026-11-13).
  Its README states **MIT** ("## License: MIT", no separate LICENSE file). It says its data derives from
  [foryVERX/Overwatch-Server-Selector](https://github.com/foryVERX/Overwatch-Server-Selector) plus community contributions;
  that repository declares no license. The ranges are factual network data (IP CIDRs); we'll remove them on request.
- svtoggle is not affiliated with Blizzard.

## Rust crates
The app is built from Rust crates, each under its own license (mostly MIT and/or Apache-2.0, plus a few others).
List them with `cargo install cargo-license && cargo license`, or generate a full attribution with
`cargo install cargo-about && cargo about generate`.
