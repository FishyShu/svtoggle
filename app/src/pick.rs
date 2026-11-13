//! Native "Open file" dialog for locating a game exe. Runs on its own thread: the shell dialog wants an
//! STA COM apartment, while the UI thread is already MTA for the firewall API.
use std::path::PathBuf;
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize};
use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
use windows::Win32::UI::Shell::{FileOpenDialog, IFileOpenDialog, SIGDN_FILESYSPATH};
use windows::core::{HSTRING, w};

/// Blocks until the user picks a file or cancels (None).
pub fn exe(title: &str) -> Option<PathBuf> {
    let title = title.to_string();
    std::thread::spawn(move || unsafe {
        let init = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let out = (|| {
            let d: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
            d.SetFileTypes(&[COMDLG_FILTERSPEC { pszName: w!("Game executable"), pszSpec: w!("*.exe") }]).ok()?;
            d.SetTitle(&HSTRING::from(title)).ok()?;
            d.Show(None).ok()?; // Err = cancelled
            let p = d.GetResult().ok()?.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
            let s = p.to_string().ok();
            CoTaskMemFree(Some(p.0 as _));
            s.map(PathBuf::from)
        })();
        if init.is_ok() {
            CoUninitialize();
        }
        out
    })
    .join()
    .ok()
    .flatten()
}
