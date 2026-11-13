//! Find each game's exe so rules can be scoped to it. Scoped rules only affect the game:
//! a region block can never break a website that happens to live in the same GCP/AWS range.
use std::path::PathBuf;
use windows::Win32::System::Registry::{HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RegGetValueW};
use windows::core::{HSTRING, PCWSTR};

pub fn exe(game: &str) -> Option<PathBuf> {
    let p = match game {
        "overwatch" => overwatch(),
        "deadlock" => deadlock(),
        _ => None,
    }?;
    p.is_file().then_some(p)
}

fn overwatch() -> Option<PathBuf> {
    let dir = reg(HKEY_LOCAL_MACHINE, r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\Overwatch", "InstallLocation")?;
    Some(PathBuf::from(dir).join(r"_retail_\Overwatch.exe"))
}

fn deadlock() -> Option<PathBuf> {
    let steam = PathBuf::from(reg(HKEY_CURRENT_USER, r"Software\Valve\Steam", "SteamPath")?);
    let vdf = std::fs::read_to_string(steam.join(r"steamapps\libraryfolders.vdf")).ok()?;
    library_paths(&vdf)
        .into_iter()
        .chain([steam])
        .map(|lib| lib.join(r"steamapps\common\Deadlock\game\bin\win64\deadlock.exe"))
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
}
