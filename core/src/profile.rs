use crate::cidr::{self, Cidr};
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct Profile {
    pub game: String,
    pub name: String,
    #[serde(default)]
    pub protect: Vec<String>,
    #[serde(rename = "region")]
    pub regions: Vec<Region>,
}

#[derive(Deserialize, Serialize, Debug, Clone, Default)]
pub struct Region {
    pub id: String,
    pub name: String,
    /// List heading this region sits under (Deadlock: POPs grouped by area). None = top level.
    pub group: Option<String>,
    #[serde(default)]
    pub lat: f32,
    #[serde(default)]
    pub lon: f32,
    /// "host:port" = TCP connect time, "icmp:IP" = echo. Used for the latency column.
    pub ping: Option<String>,
    /// Older hand-made rule covering the same servers. Counted as "blocked" and disabled on Allow.
    pub legacy_rule: Option<String>,
    #[serde(default)]
    pub cidrs: Vec<String>,
}

pub const OVERWATCH: &str = include_str!("../../profiles/overwatch.toml");
pub const DEADLOCK_SDR: &str = include_str!("../../profiles/deadlock-sdr.json");

/// Shipped profiles (no cache), in tab order. The app uses `refresh::load_all` instead.
pub fn all() -> Result<Vec<Profile>, String> {
    Ok(vec![Profile::load(OVERWATCH)?, crate::sdr::profile(DEADLOCK_SDR)?])
}

impl Profile {
    pub fn load(src: &str) -> Result<Self, String> {
        let mut p: Profile = toml::from_str(src).map_err(|e| e.to_string())?;
        p.resolve_overlaps()?;
        p.check()?;
        Ok(p)
    }

    /// Community lists mix regions up (Singapore had a /16 spanning 8 GCP regions, "EU" had Dammam).
    /// One range must belong to one region, or blocking X silently blocks part of Y. Rules:
    /// a range that strictly contains another region's range is dropped (too broad to trust);
    /// an exact duplicate stays only with the region listed first. Returns how many were dropped.
    pub fn resolve_overlaps(&mut self) -> Result<usize, String> {
        let parsed: Vec<Vec<Cidr>> = self.regions.iter().map(|r| cidr::parse_all(&r.cidrs)).collect::<Result<_, _>>()?;
        let mut dropped = 0;
        for (i, r) in self.regions.iter_mut().enumerate() {
            let keep: Vec<bool> = parsed[i].iter().map(|c| {
                !parsed.iter().enumerate().filter(|(j, _)| *j != i).any(|(j, other)| {
                    other.iter().any(|d| c.overlaps(d) && (c.len < d.len || (c == d && j < i)))
                })
            }).collect();
            let before = r.cidrs.len();
            r.cidrs = r.cidrs.iter().zip(&keep).filter(|(_, k)| **k).map(|(s, _)| s.clone()).collect();
            dropped += before - r.cidrs.len();
        }
        Ok(dropped)
    }

    pub fn region(&self, id: &str) -> Option<&Region> {
        self.regions.iter().find(|r| r.id == id)
    }

    /// Parsed + guard-checked block list for a region. Err means DO NOT apply.
    pub fn block_list(&self, id: &str) -> Result<Vec<Cidr>, String> {
        let r = self.region(id).ok_or_else(|| format!("no region '{id}' in {}", self.game))?;
        if r.cidrs.is_empty() {
            return Err(format!("{} publishes no server IPs, can't block it", r.name));
        }
        let block = cidr::parse_all(&r.cidrs)?;
        let protect = cidr::parse_all(&self.protect)?;
        let bad = cidr::violations(&block, &protect);
        if !bad.is_empty() {
            let list: Vec<String> = bad.iter().map(|(b, p)| format!("{b} hits protected {p}")).collect();
            return Err(format!("REFUSING: {}", list.join("; ")));
        }
        Ok(block)
    }

    /// Group headings in first-seen order.
    pub fn groups(&self) -> Vec<&str> {
        let mut g: Vec<&str> = Vec::new();
        for r in &self.regions {
            if let Some(x) = r.group.as_deref() {
                if !g.contains(&x) {
                    g.push(x);
                }
            }
        }
        g
    }

    pub fn check(&self) -> Result<(), String> {
        for r in self.regions.iter().filter(|r| !r.cidrs.is_empty()) {
            self.block_list(&r.id)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shipped_overwatch_profile_is_safe_and_57() {
        let p = Profile::load(OVERWATCH).unwrap();
        assert_eq!(p.block_list("me").unwrap().len(), 57);
        assert!(p.regions.iter().all(|r| r.lat != 0.0));
    }

    #[test]
    fn regions_never_share_ranges() {
        let p = Profile::load(OVERWATCH).unwrap();
        let all: Vec<(usize, Cidr)> = p.regions.iter().enumerate()
            .flat_map(|(i, r)| cidr::parse_all(&r.cidrs).unwrap().into_iter().map(move |c| (i, c))).collect();
        for (i, a) in &all {
            for (j, b) in &all {
                assert!(i == j || !a.overlaps(b), "{a} ({}) overlaps {b} ({})", p.regions[*i].id, p.regions[*j].id);
            }
        }
        let c = |s: &str| p.region(s).unwrap().cidrs.clone();
        assert!(!c("singapore").contains(&"34.124.0.0/16".into()), "8-region /16 must go");
        assert!(c("me").contains(&"34.166.0.0/16".into()) && !c("eu").contains(&"34.166.0.0/16".into()), "Dammam is ME");
        assert!(c("taiwan").contains(&"35.194.128.0/17".into()), "asia-east1 is Taiwan");
        assert!(c("na-west").contains(&"34.124.0.0/21".into()), "us-west2 is NA West");
    }

    #[test]
    fn toml_roundtrip_keeps_profile() {
        let p = Profile::load(OVERWATCH).unwrap();
        let back = Profile::load(&toml::to_string(&p).unwrap()).unwrap();
        assert_eq!(back.regions.len(), p.regions.len());
        assert_eq!(back.region("me").unwrap().legacy_rule, p.region("me").unwrap().legacy_rule);
    }

    #[test]
    fn profile_touching_protected_is_rejected() {
        let src = r#"game="x"
name="x"
protect=["100.70.0.0/16"]
[[region]]
id="a"
name="a"
cidrs=["100.70.0.0/15"]"#;
        assert!(Profile::load(src).unwrap_err().starts_with("REFUSING"));
    }
}
