//! Live server lists. Fetched lists are cached in %APPDATA%\svtoggle and win over the shipped ones.
//! Nothing here touches the firewall: callers re-apply blocked regions after a refresh.
use crate::cidr::{self, Cidr};
use crate::profile::{self, Profile};
use crate::sdr;
use std::collections::BTreeSet;
use std::path::PathBuf;

const QIIKS: &str = "https://raw.githubusercontent.com/Qiiks/ow2-server-list/main/ow2-servers.json";
// Qiiks key -> our region id. "me" is ours (hand-verified list), never overwritten.
const OW_MAP: &[(&str, &str)] = &[
    ("OW2-NA-East", "na-east"), ("OW2-NA-Central", "na-central"), ("OW2-NA-West", "na-west"),
    ("OW2-EU", "eu"), ("OW2-Brazil", "brazil"), ("OW2-Singapore", "singapore"), ("OW2-Japan", "japan"),
    ("OW2-Korea", "korea"), ("OW2-Taiwan", "taiwan"), ("OW2-Australia", "australia"),
];

/// %APPDATA%\svtoggle, or `svtoggle-data` next to the exe when the exe's name contains "portable"
/// (rename svtoggle-gui.exe to svtoggle-portable.exe and it carries its presets/cache on a USB stick).
pub fn data_dir() -> PathBuf {
    if let Some(dir) = portable_dir(std::env::current_exe().ok()) {
        return dir;
    }
    let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    base.join("svtoggle")
}

fn portable_dir(exe: Option<PathBuf>) -> Option<PathBuf> {
    let exe = exe?;
    let name = exe.file_stem()?.to_string_lossy().to_lowercase();
    name.contains("portable").then(|| exe.with_file_name("svtoggle-data"))
}

fn cache(name: &str) -> Option<String> {
    std::fs::read_to_string(data_dir().join(name)).ok()
}

fn save(name: &str, body: &str) -> Result<(), String> {
    std::fs::create_dir_all(data_dir()).map_err(|e| e.to_string())?;
    std::fs::write(data_dir().join(name), body).map_err(|e| e.to_string())
}

/// Cached profile if it loads and passes the guard, else the shipped one.
pub fn load_all() -> Vec<Profile> {
    let [ow0, dl0]: [Profile; 2] = profile::all().expect("shipped profiles are covered by tests").try_into().expect("2 games");
    // cache holds only the fetched IP lists; names/coords/ping/guard always come from the shipped profile
    let ow = cache(OW_CACHE).and_then(|s| overlay(&ow0, &s).ok());
    let dl = cache("deadlock-sdr.json").and_then(|s| sdr::profile(&s).ok());
    let mut out = vec![ow.unwrap_or(ow0), dl.unwrap_or(dl0)];
    let mine = user_protect();
    for p in &mut out {
        apply_protect(p, &mine);
    }
    out
}

/// Personal never-block list, `protect.txt` in the data dir (one CIDR per line, # comments).
/// Lives outside the repo on purpose: it's specific to where the user plays from.
pub fn user_protect() -> Vec<String> {
    cache("protect.txt")
        .map(|s| s.lines().map(|l| l.split('#').next().unwrap_or("").trim().to_string()).filter(|l| !l.is_empty()).collect())
        .unwrap_or_default()
}

/// Malformed lines of protect.txt (they're skipped by `apply_protect`, so the user should hear about them).
pub fn protect_problems() -> Vec<String> {
    cache("protect.txt").map(|s| problems(&s)).unwrap_or_default()
}

fn problems(src: &str) -> Vec<String> {
    src.lines()
        .enumerate()
        .filter_map(|(i, l)| {
            let l = l.split('#').next().unwrap_or("").trim();
            (!l.is_empty() && Cidr::parse(l).is_err()).then(|| format!("line {}: '{l}' is not a CIDR", i + 1))
        })
        .collect()
}

/// Add `extra` to the profile's guard and strip every region range that touches it. Bad lines are
/// skipped rather than failing the load; the block-time guard still refuses anything overlapping.
pub fn apply_protect(p: &mut Profile, extra: &[String]) -> usize {
    let cidrs: Vec<Cidr> = extra.iter().filter_map(|s| Cidr::parse(s).ok()).collect();
    p.protect.extend(cidrs.iter().map(|c| c.to_string()));
    let mut dropped = 0;
    for r in &mut p.regions {
        let before = r.cidrs.len();
        r.cidrs.retain(|s| Cidr::parse(s).map_or(true, |c| !cidrs.iter().any(|x| c.overlaps(x))));
        dropped += before - r.cidrs.len();
    }
    dropped
}

const OW_CACHE: &str = "overwatch-lists.json";

/// Shipped profile with cached per-region CIDR lists swapped in. Guard re-runs, so a bad cache is rejected.
fn overlay(base: &Profile, json: &str) -> Result<Profile, String> {
    let lists: std::collections::BTreeMap<String, Vec<String>> = serde_json::from_str(json).map_err(|e| e.to_string())?;
    let mut p = base.clone();
    for r in p.regions.iter_mut().filter(|r| r.id != "me") {
        if let Some(l) = lists.get(&r.id).filter(|l| !l.is_empty()) {
            r.cidrs = l.clone();
        }
    }
    p.resolve_overlaps()?;
    p.check()?;
    Ok(p)
}

fn get(url: &str) -> Result<String, String> {
    let mut r = ureq::get(url).call().map_err(|e| format!("{url}: {e}"))?;
    r.body_mut().with_config().limit(20 << 20).read_to_string().map_err(|e| e.to_string())
}

/// "+N / -M ranges" between two versions of a profile.
pub fn diff(old: &Profile, new: &Profile) -> String {
    let set = |p: &Profile| -> BTreeSet<String> { p.regions.iter().flat_map(|r| r.cidrs.clone()).collect() };
    let (a, b) = (set(old), set(new));
    format!("+{} / -{} ranges", b.difference(&a).count(), a.difference(&b).count())
}

pub fn refresh(game: &str, current: &Profile) -> Result<(Profile, String), String> {
    match game {
        "deadlock" => {
            let body = get(sdr::URL)?;
            let mut p = sdr::profile_checked(&body)?;
            apply_protect(&mut p, &user_protect());
            save("deadlock-sdr.json", &body)?;
            let rev = sdr::revision(&body).unwrap_or(0);
            Ok((p.clone(), format!("Deadlock relays rev {rev}: {}", diff(current, &p))))
        }
        "overwatch" => {
            let body = get(QIIKS)?;
            let (p, dropped) = merge_qiiks(current, &body)?;
            let lists: std::collections::BTreeMap<&str, &Vec<String>> = p.regions.iter().map(|r| (r.id.as_str(), &r.cidrs)).collect();
            save(OW_CACHE, &serde_json::to_string(&lists).map_err(|e| e.to_string())?)?;
            Ok((p.clone(), format!("Overwatch lists: {}, {dropped} protected ranges skipped", diff(current, &p))))
        }
        _ => Err(format!("unknown game {game}")),
    }
}

const MIN_OW_REGIONS: usize = 8;

/// Replace each mapped region's CIDRs with Qiiks' list, minus anything touching `protect`.
/// Errs on implausible data (too few regions, a region with an empty list) so a broken upstream
/// can't wipe the cache.
fn merge_qiiks(current: &Profile, body: &str) -> Result<(Profile, usize), String> {
    let v: serde_json::Value = serde_json::from_str(body).map_err(|e| e.to_string())?;
    let present = OW_MAP.iter().filter(|(k, _)| v["regions"][k]["cidrs"].as_array().is_some_and(|a| !a.is_empty())).count();
    if present < MIN_OW_REGIONS {
        return Err(format!("Qiiks list has only {present} of {} regions (expected {MIN_OW_REGIONS}+), keeping the old lists", OW_MAP.len()));
    }
    let protect = cidr::parse_all(&current.protect)?;
    let mut p = current.clone();
    let mut dropped = 0;
    let mut updated = Vec::new();
    for (key, id) in OW_MAP {
        let Some(list) = v["regions"][key]["cidrs"].as_array() else { continue };
        if list.is_empty() {
            return Err(format!("Qiiks list for {key} is empty, keeping the old lists"));
        }
        let mut keep = Vec::new();
        for s in list.iter().filter_map(|x| x.as_str()) {
            let c = Cidr::parse(s)?; // a malformed entry fails the whole refresh
            if protect.iter().any(|pr| c.overlaps(pr)) {
                dropped += 1; // shortcut: drops a whole range that merely contains a protected one; fine while ranges are this granular
            } else {
                keep.push(c.to_string());
            }
        }
        if let Some(r) = p.regions.iter_mut().find(|r| r.id == *id) {
            if !keep.is_empty() {
                r.cidrs = keep;
                updated.push(*id);
            }
        }
    }
    p.resolve_overlaps()?;
    // overlap resolution can still empty a region (all its ranges claimed by another region)
    if let Some(r) = p.regions.iter().find(|r| updated.contains(&r.id.as_str()) && r.cidrs.is_empty()) {
        return Err(format!("new Overwatch list for {} is empty, keeping the old lists", r.name));
    }
    p.check()?;
    Ok((p, dropped))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Qiiks-shaped body: every mapped region gets its own 198.18.i.0/24, `eu` and `skip` override that.
    fn qiiks(eu: &str, skip: &[&str]) -> String {
        let mut regions: Vec<String> = OW_MAP
            .iter()
            .enumerate()
            .filter(|(_, (k, _))| !skip.contains(k))
            .map(|(i, (k, _))| {
                let list = if *k == "OW2-EU" { eu.to_string() } else { format!(r#"["198.18.{i}.0/24"]"#) };
                format!(r#""{k}":{{"cidrs":{list}}}"#)
            })
            .collect();
        regions.push(r#""OW2-MiddleEast":{"cidrs":["1.2.3.0/24"]}"#.into());
        format!(r#"{{"regions":{{{}}}}}"#, regions.join(","))
    }

    #[test]
    fn qiiks_merge_strips_protected_and_keeps_me() {
        let mut cur = Profile::load(profile::OVERWATCH).unwrap();
        cur.protect = vec!["100.70.0.0/16".into()];
        let (p, dropped) = merge_qiiks(&cur, &qiiks(r#"["100.70.0.0/16","104.155.0.0/17"]"#, &[])).unwrap();
        assert_eq!(dropped, 1);
        assert_eq!(p.region("eu").unwrap().cidrs, vec!["104.155.0.0/17"]);
        assert_eq!(p.region("japan").unwrap().cidrs, vec!["198.18.6.0/24"]);
        assert_eq!(p.region("me").unwrap().cidrs.len(), 57);
    }

    #[test]
    fn qiiks_merge_rejects_implausible_data() {
        let cur = Profile::load(profile::OVERWATCH).unwrap();
        let eu = r#"["104.155.0.0/17"]"#;
        // two regions missing is tolerated, the old list stays
        let (p, _) = merge_qiiks(&cur, &qiiks(eu, &["OW2-Korea", "OW2-Taiwan"])).unwrap();
        assert_eq!(p.region("korea").unwrap().cidrs, cur.region("korea").unwrap().cidrs);
        assert!(merge_qiiks(&cur, &qiiks(eu, &["OW2-Korea", "OW2-Taiwan", "OW2-Japan"])).unwrap_err().contains("only 7"));
        assert!(merge_qiiks(&cur, &qiiks("[]", &[])).unwrap_err().contains("OW2-EU"));
        assert!(merge_qiiks(&cur, "{}").is_err());
    }

    #[test]
    fn protect_parser_reports_bad_lines() {
        let src = "# mine\n100.70.0.0/16\n\n abc # typo\n1.2.3.4\n1.2.3.0/33\n";
        assert_eq!(problems(src), vec!["line 4: 'abc' is not a CIDR", "line 6: '1.2.3.0/33' is not a CIDR"]);
        assert!(problems("").is_empty());
    }

    #[test]
    fn user_protect_strips_and_guards() {
        let mut p = Profile::load(profile::OVERWATCH).unwrap();
        let eu0 = p.region("eu").unwrap().cidrs[0].clone();
        assert_eq!(apply_protect(&mut p, &[eu0.clone(), "garbage".into()]), 1);
        assert!(!p.region("eu").unwrap().cidrs.contains(&eu0));
        assert!(p.protect.contains(&eu0), "block-time guard knows it too");
    }

    #[test]
    fn portable_name_keeps_data_next_to_exe() {
        let p = |s: &str| portable_dir(Some(PathBuf::from(s)));
        assert_eq!(p(r"E:\usb\svtoggle-portable.exe"), Some(PathBuf::from(r"E:\usb\svtoggle-data")));
        assert_eq!(p(r"C:\Program Files\svtoggle\svtoggle-gui.exe"), None);
    }

    #[test]
    fn overlay_swaps_lists_but_not_me_or_metadata() {
        let mut base = Profile::load(profile::OVERWATCH).unwrap();
        base.protect = vec!["100.70.0.0/16".into()];
        let p = overlay(&base, r#"{"eu":["104.155.0.0/17"],"me":["1.2.3.0/24"]}"#).unwrap();
        assert_eq!(p.region("eu").unwrap().cidrs, vec!["104.155.0.0/17"]);
        assert_eq!(p.region("me").unwrap().cidrs.len(), 57);
        assert_eq!(p.region("me").unwrap().ping, base.region("me").unwrap().ping);
        assert!(overlay(&base, r#"{"eu":["100.70.0.0/16"]}"#).is_err()); // poisoned cache -> shipped list
    }
}
