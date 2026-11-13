//! Saved combos of blocked regions, stored in %APPDATA%\svtoggle\presets.toml.
use crate::firewall::Fw;
use crate::profile::Profile;
use crate::refresh::data_dir;
use crate::toggle;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Preset {
    pub name: String,
    pub game: String,
    pub blocked: Vec<String>,
}

#[derive(Deserialize, Serialize, Default)]
struct File {
    #[serde(default, rename = "preset")]
    presets: Vec<Preset>,
}

pub fn load() -> Vec<Preset> {
    std::fs::read_to_string(data_dir().join("presets.toml"))
        .ok()
        .and_then(|s| toml::from_str::<File>(&s).ok())
        .map(|f| f.presets)
        .unwrap_or_default()
}

pub fn save(presets: &[Preset]) -> Result<(), String> {
    let s = toml::to_string(&File { presets: presets.to_vec() }).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(data_dir()).map_err(|e| e.to_string())?;
    std::fs::write(data_dir().join("presets.toml"), s).map_err(|e| e.to_string())
}

/// Make the firewall match the preset exactly: listed regions blocked, every other region allowed.
/// Keeps going past failures so one bad region doesn't leave the rest half-applied; returns all errors.
pub fn apply(fw: &Fw, prof: &Profile, blocked: &[String]) -> Vec<String> {
    let mut errs = Vec::new();
    for r in &prof.regions {
        let want = blocked.contains(&r.id);
        if want == toggle::is_blocked(fw, prof, &r.id) {
            continue;
        }
        if let Err(e) = toggle::set(fw, prof, &r.id, want) {
            errs.push(format!("{}: {e}", r.name));
        }
    }
    errs
}

/// Region ids currently blocked, for "save current as preset".
pub fn snapshot(fw: &Fw, prof: &Profile) -> Vec<String> {
    prof.regions.iter().filter(|r| toggle::is_blocked(fw, prof, &r.id)).map(|r| r.id.clone()).collect()
}
