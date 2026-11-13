//! One svtoggle at a time. A second launch pokes the first one (which shows its window) and exits.
use eframe::egui;
use std::sync::atomic::{AtomicIsize, Ordering};
use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError, HANDLE};
use windows::Win32::System::Threading::{CreateEventW, CreateMutexW, INFINITE, SetEvent, WaitForSingleObject};
use windows::core::w;

// Local\ = per login session, so two users on one PC don't fight over it
const MUTEX: windows::core::PCWSTR = w!("Local\\svtoggle-single-instance");
const SHOW: windows::core::PCWSTR = w!("Local\\svtoggle-show");

static EVENT: AtomicIsize = AtomicIsize::new(0);

/// True if we're the first instance. False = another one is running and has been asked to show itself.
pub fn claim() -> bool {
    unsafe {
        // event BEFORE mutex: whoever sees the mutex taken is guaranteed to find the event to signal
        let Ok(ev) = CreateEventW(None, false, false, SHOW) else { return true };
        EVENT.store(ev.0 as isize, Ordering::Relaxed);
        // leaked on purpose: the mutex must live as long as the process
        let m = CreateMutexW(None, false, MUTEX);
        if m.is_err() || GetLastError() != ERROR_ALREADY_EXISTS {
            return true; // no mutex at all = can't tell, so run rather than silently do nothing
        }
        let _ = SetEvent(ev);
        false
    }
}

/// First instance: wait for pokes from later launches and bring the window back.
pub fn listen(ctx: &egui::Context) {
    let ev = EVENT.load(Ordering::Relaxed); // HANDLE isn't Send; the raw value is fine across threads
    if ev == 0 {
        return;
    }
    let ctx = ctx.clone();
    std::thread::spawn(move || loop {
        unsafe { WaitForSingleObject(HANDLE(ev as _), INFINITE) };
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        ctx.request_repaint();
    });
}
