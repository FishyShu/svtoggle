//! Find each game's exe so rules can be scoped to it. Scoped rules only affect the game:
//! a region block can never break a website that happens to live in the same GCP/AWS range.
use crate::refresh::data_dir;
use std::path::{Path, PathBuf};
use windows::Win32::System::Registry::{HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RegGetValueW};
use windows::core::{HSTRING, PCWSTR};

/// The game's exe: the user's override if set (and still there), else auto-detection.
pub fn exe(game: &str) -> Option<PathBuf> {
    let over = std::fs::read_to_string(override_file(game)).ok().map(|s| PathBuf::from(s.trim()));
    if let Some(p) = over.filter(|p| p.is_file() && name_ok(game, p)) {
        return Some(p);
    }
    let p = match game {
        "overwatch" => overwatch(),
        "deadlock" => deadlock(),
        _ => None,
    }?;
    p.is_file().then_some(p)
}

/// Point a game at an exe detection missed. The file must exist and be named like the real exe.
pub fn set_override(game: &str, path: &Path) -> Result<(), String> {
    let want = exe_name(game).ok_or_else(|| format!("unknown game {game}"))?;
    if !path.is_file() {
        return Err(format!("{} does not exist", path.display()));
    }
    if !name_ok(game, path) {
        return Err(format!("expected {want}, got {}", path.display()));
    }
    std::fs::create_dir_all(data_dir()).map_err(|e| e.to_string())?;
    std::fs::write(override_file(game), path.to_string_lossy().as_bytes()).map_err(|e| e.to_string())
}

pub fn clear_override(game: &str) -> Result<(), String> {
    match std::fs::remove_file(override_file(game)) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
        _ => Ok(()),
    }
}

fn override_file(game: &str) -> PathBuf {
    data_dir().join(format!("exe-{game}.txt"))
}

fn exe_name(game: &str) -> Option<&'static str> {
    match game {
        "overwatch" => Some("Overwatch.exe"),
        "deadlock" => Some("deadlock.exe"),
        _ => None,
    }
}

fn name_ok(game: &str, path: &Path) -> bool {
    let name = path.file_name().map(|n| n.to_string_lossy().to_string());
    exe_name(game).zip(name).is_some_and(|(want, got)| want.eq_ignore_ascii_case(&got))
}

fn overwatch() -> Option<PathBuf> {
    let bnet = reg(HKEY_LOCAL_MACHINE, r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\Overwatch", "InstallLocation")
        .map(|dir| PathBuf::from(dir).join(r"_retail_\Overwatch.exe"))
        .filter(|p| p.is_file());
    // Steam (appid 2357570). shortcut: folder name not verified on a real install, so both spellings are tried
    bnet.or_else(|| steam_find(&[r"Overwatch\_retail_\Overwatch.exe", r"Overwatch 2\_retail_\Overwatch.exe"]))
}

fn deadlock() -> Option<PathBuf> {
    steam_find(&[r"Deadlock\game\bin\win64\deadlock.exe"])
}

/// First existing `steamapps\common\<rel>` across every Steam library.
fn steam_find(rels: &[&str]) -> Option<PathBuf> {
    let steam = PathBuf::from(reg(HKEY_CURRENT_USER, r"Software\Valve\Steam", "SteamPath")?);
    let vdf = std::fs::read_to_string(steam.join(r"steamapps\libraryfolders.vdf")).unwrap_or_default();
    library_paths(&vdf)
        .into_iter()
        .chain([steam])
        .flat_map(|lib| rels.iter().map(move |r| lib.join(r"steamapps\common").join(r)))
        .find(|p| p.is_file())
}

/// `"path"		"D:\\SteamLibrary"` lines from libraryfolders.vdf.
fn library_paths(vdf: &str) -> Vec<PathBuf> {
    vdf.lines()
        .filter_map(|l| {
            let l = l.trim().strip_prefix("\"path\"")?.trim().trim_matches('"');
            Some(PathBuf::from(l.replace(r"\\", r"\")))
        })
        .collect()
}

fn reg(root: HKEY, key: &str, val: &str) -> Option<String> {
    let mut buf = [0u16; 1024];
    let mut len = (buf.len() * 2) as u32;
    unsafe {
        RegGetValueW(root, PCWSTR(HSTRING::from(key).as_ptr()), PCWSTR(HSTRING::from(val).as_ptr()),
                     RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr().cast()), Some(&mut len)).ok().ok()?;
    }
    let n = (len as usize / 2).saturating_sub(1); // drop the NUL
    Some(String::from_utf16_lossy(&buf[..n]))
}

#[cfg(test)]
mod tests {
    #[test]
    fn parses_vdf_paths() {
        let v = "\"libraryfolders\"\n{\n\t\"0\"\n\t{\n\t\t\"path\"\t\t\"D:\\\\SteamLibrary\"\n";
        assert_eq!(super::library_paths(v), vec![std::path::PathBuf::from(r"D:\SteamLibrary")]);
    }

    #[test]
    fn override_name_must_match_game_exe() {
        let ok = |g: &str, p: &str| super::name_ok(g, std::path::Path::new(p));
        assert!(ok("overwatch", r"D:\Games\Overwatch\_retail_\OVERWATCH.EXE"));
        assert!(ok("deadlock", r"E:\x\deadlock.exe"));
        assert!(!ok("deadlock", r"E:\x\Overwatch.exe"));
        assert!(!ok("overwatch", r"D:\Overwatch\Overwatch Launcher.exe"));
        assert!(!ok("nope", r"E:\x\deadlock.exe"));
    }
}
