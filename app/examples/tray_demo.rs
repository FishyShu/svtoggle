//! Integration sketch: cargo run -p svtoggle --example tray_demo
use eframe::egui;
use svtoggle::tray::{Tray, TrayCmd, handle_close_to_tray};

struct Demo {
    tray: Tray,
    quitting: bool,
    log: Vec<String>,
}

impl eframe::App for Demo {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        for cmd in self.tray.poll() {
            self.log.push(format!("{cmd:?}"));
            if cmd == TrayCmd::Quit {
                self.quitting = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            } // Show is handled by the tray thread; AllowAll/Preset: call core here.
        }
        if !self.quitting {
            handle_close_to_tray(&ctx);
        }
        ui.label("Close me: I hide to tray. Tray menu: Quit exits.");
        for l in &self.log {
            ui.label(l);
        }
    }
}

fn main() -> eframe::Result {
    eframe::run_native(
        "tray demo",
        eframe::NativeOptions::default(),
        Box::new(|cc| {
            let tray = Tray::new(&cc.egui_ctx, &["Tilt".into(), "Quiet".into()])?;
            Ok(Box::new(Demo { tray, quitting: false, log: vec![] }))
        }),
    )
}
