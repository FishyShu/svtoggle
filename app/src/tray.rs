//! System tray icon with Show / Allow everything / Presets / Quit, plus close-to-tray.
//!
//! Create `Tray` on the main thread (inside the eframe creation closure): on Windows the
//! tray window needs the main thread's message loop, which winit runs.
use eframe::egui;
use std::sync::mpsc::{Receiver, channel};
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use tray_icon::{Icon, MouseButton, TrayIcon, TrayIconBuilder, TrayIconEvent};

#[derive(Debug, Clone, PartialEq)]
pub enum TrayCmd {
    Show,
    AllowAll,
    Preset(String),
    Quit,
}

const ID_SHOW: &str = "show";
const ID_ALL: &str = "all";
const ID_QUIT: &str = "quit";
const PRESET_PREFIX: &str = "preset:";

pub struct Tray {
    icon: TrayIcon,
    submenu: Submenu,
    rx: Receiver<TrayCmd>,
}

impl Tray {
    pub fn new(ctx: &egui::Context, presets: &[String]) -> Result<Tray, String> {
        let submenu = Submenu::new("Presets", true);
        let menu = Menu::new();
        menu.append_items(&[
            &MenuItem::with_id(ID_SHOW, "Show", true, None),
            &MenuItem::with_id(ID_ALL, "Allow everything", true, None),
            &submenu,
            &PredefinedMenuItem::separator(),
            &MenuItem::with_id(ID_QUIT, "Quit", true, None),
        ])
        .map_err(|e| e.to_string())?;
        fill(&submenu, presets);

        let icon = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(false)
            .with_tooltip("svtoggle")
            .with_icon(make_icon())
            .build()
            .map_err(|e| e.to_string())?;

        // Forward clicks off the UI thread so they work while the window is hidden (no frames run then).
        let (tx, rx) = channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let (menu_rx, tray_rx) = (MenuEvent::receiver(), TrayIconEvent::receiver());
            loop {
                let cmd = if let Ok(ev) = menu_rx.try_recv() {
                    match ev.id.0.as_str() {
                        ID_SHOW => Some(TrayCmd::Show),
                        ID_ALL => Some(TrayCmd::AllowAll),
                        ID_QUIT => Some(TrayCmd::Quit),
                        s => s.strip_prefix(PRESET_PREFIX).map(|n| TrayCmd::Preset(n.to_string())),
                    }
                } else if let Ok(ev) = tray_rx.try_recv() {
                    matches!(ev, TrayIconEvent::DoubleClick { button: MouseButton::Left, .. })
                        .then_some(TrayCmd::Show)
                } else {
                    // shortcut: 50 ms poll; swap for a blocking recv if the idle wakeups ever matter.
                    std::thread::sleep(std::time::Duration::from_millis(50));
                    continue;
                };
                let Some(cmd) = cmd else { continue };
                // a hidden window runs no frames, so poll() would never see the cmd: Quit exits here
                // (rules live in the firewall, presets are saved on edit: nothing to flush), the rest show the window
                if cmd == TrayCmd::Quit {
                    std::process::exit(0);
                }
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                if tx.send(cmd).is_err() {
                    return; // Tray dropped
                }
                ctx.request_repaint();
            }
        });
        Ok(Tray { icon, submenu, rx })
    }

    pub fn set_presets(&mut self, presets: &[String]) {
        while self.submenu.remove_at(0).is_some() {}
        fill(&self.submenu, presets);
    }

    pub fn poll(&self) -> Vec<TrayCmd> {
        self.rx.try_iter().collect()
    }

    pub fn set_tooltip(&self, text: &str) {
        let _ = self.icon.set_tooltip(Some(text));
    }
}

fn fill(sub: &Submenu, presets: &[String]) {
    for p in presets {
        let _ = sub.append(&MenuItem::with_id(format!("{PRESET_PREFIX}{p}"), p, true, None));
    }
}

/// Call once per frame. On a close request (X button) cancels the close and hides the window.
/// Returns true if it hid. For a real quit (tray Quit), set your own flag, skip this fn, and send
/// `ViewportCommand::Close`; otherwise the close is cancelled again.
pub fn handle_close_to_tray(ctx: &egui::Context) -> bool {
    if ctx.input(|i| i.viewport().close_requested()) {
        ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        return true;
    }
    false
}

/// Same mark as the exe icon.
fn make_icon() -> Icon {
    Icon::from_rgba(crate::icon::rgba(32), 32, 32).expect("32x32 rgba is valid")
}
