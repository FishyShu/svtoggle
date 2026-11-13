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

#[derive(Debug, PartialEq)]
enum Step {
    Reblock,
    Disable,
}

/// Which regions reconcile touches. Only regions whose own svtoggle rule is enabled (legacy-only = left alone).
fn plan(prev: Option<&Profile>, now: &Profile, enabled: impl Fn(&str) -> bool) -> Vec<(String, Step)> {
    let mut out: Vec<(String, Step)> = now
        .regions
        .iter()
        .filter(|r| enabled(&r.id))
        .map(|r| (r.id.clone(), if r.cidrs.is_empty() { Step::Disable } else { Step::Reblock }))
        .collect();
    if let Some(p) = prev {
        out.extend(p.regions.iter().filter(|r| now.region(&r.id).is_none() && enabled(&r.id)).map(|r| (r.id.clone(), Step::Disable)));
    }
    out
}

/// After a list refresh or protect.txt change: push the current guarded lists into every enabled rule,
/// disable rules whose region has nothing blockable left or vanished. Returns errors (empty = fine).
pub fn reconcile(fw: &Fw, prev: Option<&Profile>, now: &Profile) -> Vec<String> {
    let enabled = |id: &str| fw.status(&rule_name(&now.game, id)).is_some_and(|(en, _)| en);
    plan(prev, now, enabled)
        .into_iter()
        .filter_map(|(id, step)| {
            let res = match step {
                Step::Reblock => set(fw, now, &id, true).map(|_| ()),
                Step::Disable => fw.allow(&rule_name(&now.game, &id)).map(|_| ()).map_err(deny),
            };
            res.err().map(|e| format!("{id}: {e}"))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::Region;

    fn prof(regions: &[(&str, &[&str])]) -> Profile {
        let regions = regions
            .iter()
            .map(|(id, c)| Region { id: id.to_string(), name: id.to_string(), cidrs: c.iter().map(|s| s.to_string()).collect(), ..Default::default() })
            .collect();
        Profile { game: "g".into(), name: "G".into(), protect: vec![], regions }
    }

    #[test]
    fn plan_reblocks_enabled_disables_empty_and_gone() {
        let prev = prof(&[("a", &["1.0.0.0/8"]), ("b", &["2.0.0.0/8"]), ("gone", &["3.0.0.0/8"]), ("gone-off", &["4.0.0.0/8"])]);
        let now = prof(&[("a", &["1.0.0.0/8"]), ("b", &[]), ("c", &["5.0.0.0/8"])]);
        let on = |id: &str| ["a", "b", "gone"].contains(&id); // c, gone-off: rule not enabled
        assert_eq!(
            plan(Some(&prev), &now, on),
            vec![("a".into(), Step::Reblock), ("b".into(), Step::Disable), ("gone".into(), Step::Disable)]
        );
        assert_eq!(plan(None, &now, |_| false), vec![]);
    }
}
