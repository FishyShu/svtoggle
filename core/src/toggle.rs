//! Block/Allow one region. Shared by the CLI and the GUI so both run the same guard + scoping.
use crate::FwError;
use crate::firewall::{Fw, rule_name};
use crate::games;
use crate::profile::Profile;

pub fn is_blocked(fw: &Fw, prof: &Profile, region: &str) -> bool {
    let on = |n: &str| fw.status(n).is_some_and(|(en, _)| en);
    on(&rule_name(&prof.game, region))
        || prof.region(region).and_then(|r| r.legacy_rule.as_deref()).is_some_and(on)
}

pub fn set(fw: &Fw, prof: &Profile, region: &str, block: bool) -> Result<String, String> {
    let r = prof.region(region).ok_or_else(|| format!("no region '{region}'"))?;
    let name = rule_name(&prof.game, region);
    if block {
        let list = prof.block_list(region)?; // protect-guard runs here
        // refuse rather than fall back to an all-programs rule: that would hit unrelated traffic
        let exe = games::exe(&prof.game).ok_or_else(|| format!("{} exe not found, not blocking", prof.name))?;
        fw.block(&name, &list, &exe.to_string_lossy()).map_err(deny)?;
        if let Some(old) = &r.legacy_rule {
            fw.allow(old).map_err(deny)?; // superseded; disabled, never deleted
        }
        Ok(format!("Blocked {} ({} ranges) for {}", r.name, list.len(), exe.display()))
    } else {
        fw.allow(&name).map_err(deny)?;
        if let Some(old) = &r.legacy_rule {
            fw.allow(old).map_err(deny)?;
        }
        Ok(format!("Allowed {}", r.name))
    }
}

pub fn deny(e: FwError) -> String {
    if e.code().0 as u32 == 0x8007_0005 { "access denied: run as administrator".into() } else { e.to_string() }
}
