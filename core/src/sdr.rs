//! Deadlock profile built straight from Valve's SDR config JSON. One region per POP (city).
//! Live: https://api.steampowered.com/ISteamApps/GetSDRConfig/v1?appid=1422450
use crate::profile::{Profile, Region};
use serde::Deserialize;
use std::collections::BTreeMap;

pub const URL: &str = "https://api.steampowered.com/ISteamApps/GetSDRConfig/v1?appid=1422450";

#[derive(Deserialize)]
struct Config {
    revision: u64,
    pops: BTreeMap<String, Pop>,
}

#[derive(Deserialize)]
struct Pop {
    desc: Option<String>,
    geo: Option<[f32; 2]>, // [lon, lat]
    #[serde(default)]
    relays: Vec<Relay>,
}

#[derive(Deserialize)]
struct Relay {
    ipv4: String,
}

// POP code -> list heading. Codes Valve adds later land in "Other" instead of vanishing.
const GROUPS: &[(&str, &[&str])] = &[
    ("North America", &["iad", "atl", "ord", "dfw", "lax", "sea", "eat"]),
    ("Europe", &["ams", "lhr", "dlhr", "par", "mad", "fra", "dfra", "fsn", "vie", "dvie", "waw", "sto", "sto2", "dsto", "hel"]),
    ("Middle East", &["dxb", "dfjr"]),
    ("India", &["bom2", "maa2"]),
    ("Asia", &["hkg", "sgp", "seo", "tyo", "gum"]),
    ("Oceania", &["syd"]),
    ("South America", &["gru", "eze", "lim", "scl"]),
    ("Africa", &["jnb"]),
];

fn group_of(code: &str) -> &'static str {
    GROUPS.iter().find(|(_, c)| c.contains(&code)).map_or("Other", |(g, _)| g)
}

pub fn revision(json: &str) -> Option<u64> {
    serde_json::from_str::<Config>(json).ok().map(|c| c.revision)
}

pub fn profile(json: &str) -> Result<Profile, String> {
    let cfg: Config = serde_json::from_str(json).map_err(|e| format!("bad SDR config: {e}"))?;
    // Valve's geo for the Datapacket POPs is junk (dfra/dvie/dlhr have lat/lon swapped, dsto points at
    // Moscow). Same city as their twin without the 'd' (dfra -> fra), so borrow the twin's coords.
    let geo_of: BTreeMap<String, [f32; 2]> = cfg.pops.iter().filter_map(|(k, p)| Some((k.clone(), p.geo?))).collect();
    let mut regions: Vec<Region> = cfg
        .pops
        .into_iter()
        .map(|(code, p)| {
            let twin = code.strip_prefix('d').and_then(|t| geo_of.get(t)).filter(|_| p.desc.as_deref().is_some_and(|d| d.starts_with("Datapacket")));
            let [lon, lat] = twin.copied().or(p.geo).unwrap_or([0.0, 0.0]);
            Region {
                name: p.desc.unwrap_or_else(|| code.clone()),
                group: Some(group_of(&code).to_string()),
                lat,
                lon,
                ping: p.relays.first().map(|r| format!("icmp:{}", r.ipv4)),
                cidrs: p.relays.iter().map(|r| format!("{}/32", r.ipv4)).collect(),
                legacy_rule: None,
                id: code,
            }
        })
        .collect();
    let order = |r: &Region| GROUPS.iter().position(|(g, _)| Some(*g) == r.group.as_deref()).unwrap_or(99);
    regions.sort_by(|a, b| order(a).cmp(&order(b)).then(a.name.cmp(&b.name)));
    let p = Profile { game: "deadlock".into(), name: "Deadlock".into(), protect: vec![], regions };
    p.check()?;
    Ok(p)
}

const MIN_POPS: usize = 15;

/// `profile` plus a plausibility check for a fresh download: a truncated answer must not replace the cache.
pub fn profile_checked(json: &str) -> Result<Profile, String> {
    let p = profile(json)?;
    let n = p.regions.iter().filter(|r| !r.cidrs.is_empty()).count();
    if n < MIN_POPS {
        return Err(format!("Valve's SDR config lists only {n} POPs with relays (expected {MIN_POPS}+), keeping the old list"));
    }
    Ok(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(pops_with_relays: usize) -> String {
        let pops: Vec<String> = (0..pops_with_relays)
            .map(|i| format!(r#""p{i}":{{"desc":"P{i}","relays":[{{"ipv4":"10.0.{i}.1"}}]}}"#))
            .chain([r#""empty":{"desc":"E"}"#.to_string()])
            .collect();
        format!(r#"{{"revision":1,"pops":{{{}}}}}"#, pops.join(","))
    }

    #[test]
    fn checked_rejects_too_few_pops() {
        assert!(profile_checked(&cfg(14)).unwrap_err().contains("only 14"));
        assert_eq!(profile_checked(&cfg(15)).unwrap().regions.len(), 16);
        assert!(profile_checked(crate::profile::DEADLOCK_SDR).is_ok());
    }

    #[test]
    fn shipped_snapshot_parses_per_pop() {
        let p = profile(crate::profile::DEADLOCK_SDR).unwrap();
        assert!(p.regions.len() >= 30);
        let ams = p.region("ams").unwrap();
        assert_eq!(ams.group.as_deref(), Some("Europe"));
        assert!(ams.cidrs.iter().all(|c| c.ends_with("/32")) && !ams.cidrs.is_empty());
        assert!(ams.lat > 50.0 && ams.lon > 3.0); // geo is [lon, lat]; easy to flip
        assert!(p.region("fsn").unwrap().cidrs.is_empty()); // no relays published
        let dfra = p.region("dfra").unwrap();
        assert!(dfra.lat > 49.0 && dfra.lon < 10.0, "Datapacket Frankfurt must sit in Germany, not Somalia");
        let dsto = p.region("dsto").unwrap();
        assert!(dsto.lon < 20.0, "Datapacket Stockholm must not sit in Moscow");
        assert!(p.regions.iter().all(|r| r.group.as_deref() != Some("Other")), "new POP: add it to GROUPS");
    }
}
