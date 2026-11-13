use crate::cidr::Cidr;
use windows::Win32::Foundation::VARIANT_BOOL;
use windows::Win32::NetworkManagement::WindowsFirewall::*;
use windows::Win32::System::Com::*;
use windows::core::{BSTR, Result};

pub const GROUP: &str = "svtoggle";

pub fn rule_name(game: &str, region: &str) -> String {
    format!("svtoggle {game} {region}")
}

const ALL_PROFILES: i32 = 0x7FFF_FFFF;

pub struct Fw {
    pol: INetFwPolicy2,
    rules: INetFwRules,
}

thread_local! {
    // CoInitializeEx once per thread instead of on every open (the 5 s status poll would pile up refs)
    static COM: () = unsafe { let _ = CoInitializeEx(None, COINIT_MULTITHREADED); };
}

impl Fw {
    pub fn open() -> Result<Fw> {
        COM.with(|_| ());
        unsafe {
            let pol: INetFwPolicy2 = CoCreateInstance(&NetFwPolicy2, None, CLSCTX_INPROC_SERVER)?;
            let rules = pol.Rules()?;
            Ok(Fw { pol, rules })
        }
    }

    fn get(&self, name: &str) -> Option<INetFwRule> {
        unsafe { self.rules.Item(&BSTR::from(name)).ok() }
    }

    /// Create or update the outbound block rule for this name and set it enabled.
    /// Needs admin; on E_ACCESSDENIED the caller should say "run elevated".
    /// Scoped to `program` (the game exe) so nothing else on the PC loses those ranges.
    /// The rule is only enabled after every property is set: a failure halfway leaves it
    /// disabled (blocks nothing) instead of live with new ranges but the old/no exe scope.
    pub fn block(&self, name: &str, cidrs: &[Cidr], program: &str) -> Result<()> {
        let addrs = cidrs.iter().map(|c| c.to_string()).collect::<Vec<_>>().join(",");
        unsafe {
            let (r, new) = match self.get(name) {
                Some(r) => {
                    r.SetEnabled(VARIANT_BOOL(0))?;
                    (r, false)
                }
                None => {
                    let r: INetFwRule = CoCreateInstance(&NetFwRule, None, CLSCTX_INPROC_SERVER)?;
                    r.SetName(&BSTR::from(name))?;
                    r.SetEnabled(VARIANT_BOOL(0))?;
                    (r, true)
                }
            };
            r.SetGrouping(&BSTR::from(GROUP))?;
            r.SetDirection(NET_FW_RULE_DIR_OUT)?;
            r.SetAction(NET_FW_ACTION_BLOCK)?;
            r.SetProfiles(ALL_PROFILES)?;
            r.SetApplicationName(&BSTR::from(program))?;
            r.SetRemoteAddresses(&BSTR::from(addrs))?;
            if new {
                self.rules.Add(&r)?;
            }
            r.SetEnabled(VARIANT_BOOL(-1))
        }
    }

    /// Disable every rule in the svtoggle group, including ones for regions no profile lists anymore.
    pub fn allow_group(&self) -> Result<()> {
        unsafe { self.pol.EnableRuleGroup(ALL_PROFILES, &BSTR::from(GROUP), VARIANT_BOOL(0)) }
    }

    /// Disable, never delete: the rule (and its list) survives for the next Block.
    pub fn allow(&self, name: &str) -> Result<bool> {
        match self.get(name) {
            Some(r) => unsafe { r.SetEnabled(VARIANT_BOOL(0)).map(|_| true) },
            None => Ok(false),
        }
    }

    /// (exists, enabled, remote address string)
    pub fn status(&self, name: &str) -> Option<(bool, String)> {
        let r = self.get(name)?;
        unsafe {
            let en = r.Enabled().ok()?.as_bool();
            let a = r.RemoteAddresses().ok()?.to_string();
            Some((en, a))
        }
    }

    pub fn remove(&self, name: &str) -> Result<()> {
        unsafe { self.rules.Remove(&BSTR::from(name)) }
    }
}
