#![windows_subsystem = "windows"]

use eframe::egui::{self, Align, Align2, Color32, CornerRadius, FontId, Layout, Margin, RichText, Sense, Stroke, Vec2};
use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};
use svtoggle::map::{MapView, Marker, MarkerState};
use svtoggle::theme::{self as th, semibold};
use svtoggle::tray::{self, Tray, TrayCmd};
use svtoggle_core::firewall::Fw;
use svtoggle_core::monitor::{Monitor, Target};
use svtoggle_core::presets::{self, Preset};
use svtoggle_core::profile::{Profile, Region};
use svtoggle_core::{cidr, games, ping, refresh, toggle};

const STATUS_EVERY: Duration = Duration::from_secs(5);
const PING_EVERY: Duration = Duration::from_secs(30);
const TOAST_FOR: f32 = 6.0;
const ROW_H: f32 = 36.0;
const LIST_W: f32 = 390.0;
const VERSION: &str = env!("CARGO_PKG_VERSION");

enum Msg {
    Ping(String, String, Option<u32>),
    Refreshed(usize, Result<(Profile, String), String>),
}

type Key = (String, String); // game, region

struct Toast {
    text: String,
    err: bool,
    at: Instant,
}

struct App {
    profs: Vec<Profile>,
    exes: Vec<Option<String>>,
    tab: usize,
    toast: Option<Toast>,
    blocked: HashMap<Key, bool>,
    pings: HashMap<Key, Option<u32>>,
    last_status: Option<Instant>,
    last_ping: Option<(usize, Instant)>,
    refreshing: bool,
    pings_out: usize,
    presets: Vec<Preset>,
    naming: Option<String>, // Some = the "save preset" field is open
    expanded: HashSet<String>,
    map: MapView,
    monitor: Monitor,
    tray: Option<Tray>,
    logo: egui::TextureHandle,
    pending: Option<Pending>, // a block waiting for "you're in a match, sure?"
    deferred: Option<(usize, Option<Profile>)>, // rule re-apply held until the current match ends
    about: bool,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
}

/// Blocking actions held back while a match is detected (dropping out can count as leaving).
enum Pending {
    Regions(usize, Vec<String>),
    Preset(String),
}

fn targets(profs: &[Profile]) -> Vec<Target> {
    profs
        .iter()
        .flat_map(|p| p.regions.iter().map(move |r| (p, r)))
        .flat_map(|(p, r)| {
            cidr::parse_all(&r.cidrs).unwrap_or_default().into_iter().map(|c| Target {
                cidr: c,
                game: p.game.clone(),
                region: r.id.clone(),
            })
        })
        .collect()
}

impl App {
    fn new(ctx: &egui::Context) -> Self {
        th::install(ctx);
        let profs = refresh::load_all();
        let exes = profs.iter().map(|p| games::exe(&p.game).map(|e| e.display().to_string())).collect();
        let (tx, rx) = channel();
        let c = ctx.clone();
        let monitor = Monitor::start(targets(&profs), move || c.request_repaint());
        let presets = presets::load();
        svtoggle::single::listen(ctx);
        let tray = Tray::new(ctx, &preset_labels(&presets)).ok(); // no tray = app still works, just closes normally
        let img = egui::ColorImage::from_rgba_unmultiplied([48, 48], &svtoggle::icon::rgba(48));
        let logo = ctx.load_texture("logo", img, egui::TextureOptions::LINEAR);
        let tab = std::fs::read_to_string(refresh::data_dir().join("last-tab"))
            .ok()
            .and_then(|s| s.trim().parse::<usize>().ok())
            .filter(|t| *t < profs.len())
            .unwrap_or(0);
        let mut app = App { profs, exes, tab, toast: None, blocked: HashMap::new(), pings: HashMap::new(),
              last_status: None, last_ping: None, refreshing: false, pings_out: 0, presets, naming: None,
              expanded: HashSet::new(), map: MapView::default(), monitor, tray, logo, pending: None, deferred: None, about: false, tx, rx };
        if std::env::var_os("SVTOGGLE_SHOT").is_none() {
            app.startup_checks(); // screenshot runs are unelevated; their access-denied toast isn't the product
        }
        app
    }

    /// Existing rules may predate a list refresh or a protect.txt edit: bring them in line now,
    /// and say so if protect.txt has lines we couldn't read (silently ignoring them would be worse).
    fn startup_checks(&mut self) {
        let mut problems: Vec<String> = match Fw::open() {
            Ok(fw) => self.profs.iter().flat_map(|p| toggle::reconcile(&fw, None, p)).collect(),
            Err(e) => vec![e.to_string()],
        };
        if !problems.is_empty() {
            problems = vec![format!("Couldn't refresh your existing blocks ({})", problems.join("; "))];
        }
        let bad = refresh::protect_problems();
        if !bad.is_empty() {
            problems.push(format!("protect.txt: {}", bad.join("; ")));
        }
        if !problems.is_empty() {
            self.say(problems.join("; "), true);
        }
    }

    /// Route blocking actions through the in-match check.
    fn request_regions(&mut self, tab: usize, ids: Vec<String>, block: bool) {
        if block && self.monitor.current().is_some() {
            self.pending = Some(Pending::Regions(tab, ids));
        } else {
            self.set_regions(tab, ids, block);
        }
    }

    fn request_preset(&mut self, label: &str) {
        let blocks = self.presets.iter().find(|p| preset_label(p) == label).is_some_and(|p| !p.blocked.is_empty());
        if blocks && self.monitor.current().is_some() {
            self.pending = Some(Pending::Preset(label.to_string()));
        } else {
            self.apply_preset(label);
        }
    }

    fn locate_game(&mut self) {
        let game = self.profs[self.tab].game.clone();
        let Some(path) = svtoggle::pick::exe(&format!("Locate {}", nice_game(&game))) else { return };
        match games::set_override(&game, &path) {
            Ok(()) => {
                self.exes[self.tab] = games::exe(&game).map(|e| e.display().to_string());
                self.say(format!("Using {}", path.display()), false);
                // existing blocks still point at the old exe: move them over
                self.reapply(self.tab, None, format!("Using {}", path.display()));
            }
            Err(e) => self.say(e, true),
        }
    }

    fn say(&mut self, text: impl Into<String>, err: bool) {
        self.toast = Some(Toast { text: text.into(), err, at: Instant::now() });
    }

    fn is_blocked(&self, game: &str, id: &str) -> bool {
        self.blocked.get(&(game.to_string(), id.to_string())).copied().unwrap_or(false)
    }

    fn refresh_status(&mut self) {
        if let Ok(fw) = Fw::open() {
            for p in &self.profs {
                for r in &p.regions {
                    self.blocked.insert((p.game.clone(), r.id.clone()), toggle::is_blocked(&fw, p, &r.id));
                }
            }
        }
        if let Some(t) = &self.tray {
            let n = self.blocked.values().filter(|b| **b).count();
            t.set_tooltip(&format!("svtoggle: {n} blocked"));
        }
        self.last_status = Some(Instant::now());
    }

    /// Pings only the visible tab, off-thread.
    fn refresh_pings(&mut self, ctx: &egui::Context) {
        if self.pings_out > 0 {
            return; // one round at a time: tab-mashing can't pile up threads; next frame retries
        }
        let p = &self.profs[self.tab];
        for r in &p.regions {
            if let Some(spec) = r.ping.clone() {
                self.pings_out += 1;
                let (g, id, tx, ctx) = (p.game.clone(), r.id.clone(), self.tx.clone(), ctx.clone());
                std::thread::spawn(move || {
                    let _ = tx.send(Msg::Ping(g, id, ping::ping(&spec)));
                    ctx.request_repaint();
                });
            }
        }
        self.last_ping = Some((self.tab, Instant::now()));
    }

    fn start_refresh(&mut self, ctx: &egui::Context) {
        self.refreshing = true;
        let (i, game, cur, tx, ctx) = (self.tab, self.profs[self.tab].game.clone(), self.profs[self.tab].clone(), self.tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(Msg::Refreshed(i, refresh::refresh(&game, &cur)));
            ctx.request_repaint();
        });
    }

    /// Run firewall work, toast the outcome, re-read state.
    fn with_fw(&mut self, f: impl FnOnce(&Fw, &[Profile]) -> Result<String, String>) {
        let (text, err) = match Fw::open().map_err(|e| e.to_string()).and_then(|fw| f(&fw, &self.profs)) {
            Ok(t) => (t, false),
            Err(e) => (e, true),
        };
        self.say(text, err);
        self.refresh_status();
        self.last_ping = None; // re-ping so the column reflects the change
    }

    fn set_regions(&mut self, tab: usize, ids: Vec<String>, block: bool) {
        self.with_fw(|fw, profs| {
            let p = &profs[tab];
            let errs: Vec<String> = ids.iter().filter_map(|id| toggle::set(fw, p, id, block).err()).collect();
            let verb = if block { "Blocked" } else { "Allowed" };
            match (ids.len(), errs.is_empty()) {
                (1, true) => Ok(format!("{verb} {}", p.region(&ids[0]).map_or("", |r| &r.name))),
                (n, true) => Ok(format!("{verb} {n} cities")),
                _ => Err(errs.join("; ")),
            }
        });
    }

    fn allow_everything(&mut self) {
        self.with_fw(|fw, profs| {
            // whole group, so rules for cities that vanished from a refreshed list go too
            fw.allow_group().map_err(toggle::deny)?;
            for old in profs.iter().flat_map(|p| &p.regions).filter_map(|r| r.legacy_rule.as_deref()) {
                fw.allow(old).map_err(toggle::deny)?;
            }
            Ok("Everything allowed".into())
        });
    }

    fn apply_preset(&mut self, label: &str) {
        let Some(pr) = self.presets.iter().find(|p| preset_label(p) == label).cloned() else { return };
        self.with_fw(|fw, profs| {
            let p = profs.iter().find(|p| p.game == pr.game).ok_or("preset game missing")?;
            let errs = presets::apply(fw, p, &pr.blocked);
            if errs.is_empty() { Ok(format!("Preset \u{201c}{}\u{201d} applied", pr.name)) } else { Err(errs.join("; ")) }
        });
    }

    fn on_refreshed(&mut self, i: usize, res: Result<(Profile, String), String>) {
        self.refreshing = false;
        match res {
            Ok((p, summary)) => {
                let old = std::mem::replace(&mut self.profs[i], p);
                self.monitor.set_targets(targets(&self.profs));
                self.reapply(i, Some(old), summary);
                self.last_ping = None;
            }
            Err(e) => self.say(format!("Refresh failed, kept the old list: {e}"), true),
        }
    }

    /// Same reconcile as startup and the CLI (new lists into live rules, vanished regions off), but never
    /// during a detected match: new endpoints could drop the game. Held in `deferred` until it ends.
    fn reapply(&mut self, i: usize, old: Option<Profile>, summary: String) {
        if self.monitor.current().is_some() {
            self.deferred = Some((i, old));
            self.say(format!("{summary}. Your blocks update after this match."), false);
            return;
        }
        let errs = match Fw::open() {
            Ok(fw) => toggle::reconcile(&fw, old.as_ref(), &self.profs[i]),
            Err(e) => vec![e.to_string()],
        };
        self.refresh_status();
        if errs.is_empty() {
            self.say(summary, false);
        } else {
            self.say(format!("{summary}, but re-applying rules failed: {}", errs.join("; ")), true);
        }
    }

    fn save_presets(&mut self) {
        if let Err(e) = presets::save(&self.presets) {
            self.say(format!("Couldn't save presets: {e}"), true);
        }
        if let Some(t) = &mut self.tray {
            t.set_presets(&preset_labels(&self.presets));
        }
    }

    // ---------- views ----------

    fn top_bar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.horizontal_centered(|ui| {
            ui.add(egui::Image::new(&self.logo).fit_to_exact_size(Vec2::splat(22.0)));
            ui.label(RichText::new("svtoggle").font(FontId::new(16.0, semibold())));
            ui.label(RichText::new(format!("v{VERSION}")).small().color(th::FAINT));
            ui.add_space(20.0);
            // segmented game switcher
            egui::Frame::new().fill(th::BG).corner_radius(CornerRadius::same(th::R + 2)).inner_margin(Margin::same(3)).show(ui, |ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                for i in 0..self.profs.len() {
                    let sel = i == self.tab;
                    let txt = RichText::new(&self.profs[i].name).color(if sel { th::TEXT } else { th::DIM });
                    let b = egui::Button::new(txt)
                        .fill(if sel { th::RAISED } else { Color32::TRANSPARENT })
                        .stroke(if sel { Stroke::new(1.0, th::STROKE) } else { Stroke::NONE })
                        .min_size(Vec2::new(96.0, 26.0));
                    if ui.add(b).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                        self.tab = i;
                        // best effort: losing the remembered tab is harmless
                        let _ = std::fs::create_dir_all(refresh::data_dir())
                            .and_then(|_| std::fs::write(refresh::data_dir().join("last-tab"), i.to_string()));
                    }
                }
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let allow = egui::Button::new(RichText::new("Allow everything").color(th::OK)).stroke(Stroke::new(1.0, th::OK.gamma_multiply(0.45)));
                if ui.add(allow).on_hover_text("Turn off every svtoggle rule for both games").clicked() {
                    self.allow_everything();
                }
                if th::ghost(ui, RichText::new("About").color(th::DIM)).clicked() {
                    self.about = true;
                }
                if self.refreshing {
                    ui.add(egui::Spinner::new().size(14.0).color(th::DIM));
                    ui.label(RichText::new("Refreshing").color(th::DIM));
                } else if th::ghost(ui, RichText::new("Refresh lists").color(th::DIM))
                    .on_hover_text("Download the latest server lists for this game")
                    .clicked()
                {
                    self.start_refresh(ctx);
                }
            });
        });
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_centered(|ui| {
            if let Some(t) = &self.toast {
                let age = t.at.elapsed().as_secs_f32();
                let a = (TOAST_FOR + 0.6 - age).clamp(0.0, 0.6) / 0.6;
                if a > 0.0 {
                    let c = if t.err { th::BAD } else { th::TEXT };
                    ui.label(RichText::new(&t.text).color(c.gamma_multiply(a)));
                    if age > TOAST_FOR {
                        ui.ctx().request_repaint_after(Duration::from_millis(33)); // fade out
                    } else {
                        ui.ctx().request_repaint_after(Duration::from_secs_f32(TOAST_FOR - age));
                    }
                }
            }
            let mut locate = false;
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let found = self.exes[self.tab].is_some();
                let btn = RichText::new(if found { "Change game path" } else { "Locate game\u{2026}" }).small();
                locate = th::ghost(ui, if found { btn.color(th::FAINT) } else { btn.color(th::TEXT) })
                    .on_hover_text("Pick the game's exe yourself (e.g. a custom or Steam install)")
                    .clicked();
                let (note, c) = match &self.exes[self.tab] {
                    Some(e) => {
                        let exe = e.rsplit('\\').next().unwrap_or(e);
                        let ping = if self.profs[self.tab].game == "overwatch" { "ping: estimated, nearest AWS region" } else { "ping: Valve relay" };
                        (format!("rules apply to {exe} only  \u{b7}  {ping}"), th::FAINT)
                    }
                    None => ("game not found: blocking disabled".into(), th::BAD),
                };
                let full = self.exes[self.tab].clone().unwrap_or_default();
                ui.label(RichText::new(note).small().color(c)).on_hover_text(full);
            });
            if locate {
                self.locate_game();
            }
        });
    }

    /// "You're in a match" gate for anything that blocks.
    fn confirm_modal(&mut self, ctx: &egui::Context) {
        if self.pending.is_none() {
            return;
        }
        let mut choice = None;
        egui::Modal::new(egui::Id::unique("in-match-confirm")).show(ctx, |ui| {
            ui.set_width(360.0);
            ui.heading("You're in a match");
            ui.add_space(4.0);
            ui.label(RichText::new("Blocking now can drop you from this game, and leaving a competitive match can get you a penalty. Do it between matches unless you're sure.").color(th::DIM));
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if ui.button("Wait").clicked() {
                    choice = Some(false);
                }
                if ui.add(egui::Button::new(RichText::new("Block anyway").color(th::BAD))).clicked() {
                    choice = Some(true);
                }
            });
        });
        match (choice, self.pending.take()) {
            (Some(true), Some(Pending::Regions(tab, ids))) => self.set_regions(tab, ids, true),
            (Some(true), Some(Pending::Preset(label))) => self.apply_preset(&label),
            (Some(false), _) => self.say("Nothing changed", false),
            (None, keep) => self.pending = keep, // still open
            (Some(true), None) => {}
        }
    }

    fn about_window(&mut self, ctx: &egui::Context) {
        let mut open = true;
        egui::Window::new("About svtoggle").open(&mut open).collapsible(false).resizable(true)
            .default_size([520.0, 420.0]).show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.add(egui::Image::new(&self.logo).fit_to_exact_size(Vec2::splat(40.0)));
                    ui.vertical(|ui| {
                        ui.label(RichText::new(format!("svtoggle {VERSION}")).font(FontId::new(16.0, semibold())));
                        ui.label(RichText::new("Deadlock & Overwatch server selector \u{b7} MIT license").color(th::DIM));
                    });
                });
                ui.hyperlink_to("github.com/FishyShu/deadlock-overwatch-server-selector", "https://github.com/FishyShu/deadlock-overwatch-server-selector");
                ui.label(RichText::new(format!("Settings & cache: {}", refresh::data_dir().display())).small().color(th::FAINT));
                ui.separator();
                ui.label(RichText::new("Third-party notices").strong());
                egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                    ui.label(RichText::new(include_str!("../../../THIRD-PARTY-NOTICES.md")).small().color(th::DIM));
                    ui.add_space(8.0);
                    ui.label(RichText::new(include_str!("../../assets/fonts/OFL.txt")).small().monospace().color(th::FAINT));
                });
            });
        self.about = open;
    }

    fn live_card(&self, ui: &mut egui::Ui) {
        let Some(c) = self.monitor.current() else {
            let running = self.monitor.running_games();
            let line = if running.is_empty() { "No game running".to_string() } else { format!("{} running, not in a match", nice_game(&running[0])) };
            ui.label(RichText::new(line).small().color(th::FAINT));
            if let Some(e) = self.monitor.error() {
                ui.label(RichText::new(format!("match detection: {e}")).small().color(th::FAINT));
            }
            return;
        };
        let p = self.profs.iter().find(|p| p.game == c.game);
        let name = p.and_then(|p| p.region(&c.region)).map_or(c.region.clone(), |r| r.name.clone());
        egui::Frame::new().fill(th::RAISED).stroke(Stroke::new(1.0, th::ACCENT.gamma_multiply(0.35)))
            .corner_radius(CornerRadius::same(th::R)).inner_margin(Margin::symmetric(12, 10)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
                    let t = (ui.input(|i| i.time) % 1.6) as f32 / 1.6;
                    ui.painter().circle_filled(r.center(), 3.5 + 2.5 * t, th::ACCENT.gamma_multiply(0.5 * (1.0 - t)));
                    ui.painter().circle_filled(r.center(), 3.5, th::ACCENT);
                    ui.ctx().request_repaint_after(Duration::from_millis(33));
                    ui.label(RichText::new("IN A MATCH").small().color(th::ACCENT));
                });
                ui.label(RichText::new(format!("{} \u{b7} {name}", nice_game(&c.game))).font(FontId::new(15.0, semibold())));
                ui.label(RichText::new(format!("{}  \u{b7}  {} pkt/s", c.remote, c.pps)).monospace().color(th::DIM));
            });
    }

    /// One list row. Returns Some(block?) when its switch was clicked.
    fn region_row(&self, ui: &mut egui::Ui, p: &Profile, r: &Region, can_block: bool, indent: f32) -> Option<bool> {
        let blocked = self.is_blocked(&p.game, &r.id);
        let live = self.monitor.current().is_some_and(|c| c.game == p.game && c.region == r.id);
        let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_H), Sense::hover());
        let painter = ui.painter_at(rect);
        if resp.hovered() {
            painter.rect_filled(rect, CornerRadius::same(th::R), th::HOVER);
        }
        let mut x = rect.left() + 10.0 + indent;
        if live {
            painter.circle_filled(egui::pos2(x + 3.0, rect.center().y), 3.5, th::ACCENT);
            x += 12.0;
        }
        let name_c = if blocked { th::DIM } else { th::TEXT };
        // clip long names before the ping column instead of drawing under it
        let name_clip = egui::Rect::from_min_max(rect.min, egui::pos2(rect.right() - 128.0, rect.max.y));
        painter.with_clip_rect(name_clip).text(egui::pos2(x, rect.center().y), Align2::LEFT_CENTER, &r.name, FontId::proportional(14.0), name_c);
        resp.on_hover_text(format!("{} \u{b7} {} IP ranges", r.name, r.cidrs.len()));
        let ping = if r.cidrs.is_empty() {
            Some(("no IPs".to_string(), th::FAINT))
        } else if blocked {
            Some(("blocked".to_string(), th::BAD.gamma_multiply(0.85)))
        } else {
            match (r.ping.is_some(), self.pings.get(&(p.game.clone(), r.id.clone()))) {
                // ~ = estimate: Overwatch pings hit the nearest AWS region, not the game server
                (true, Some(Some(ms))) => Some((format!("{}{ms} ms", if p.game == "overwatch" { "~" } else { "" }), th::ping_color(*ms))),
                (true, Some(None)) => Some(("timeout".into(), th::FAINT)),
                (true, None) => Some(("\u{2026}".into(), th::FAINT)),
                (false, _) => Some(("\u{2014}".into(), th::FAINT)),
            }
        };
        if let Some((t, c)) = ping {
            painter.text(egui::pos2(rect.right() - 58.0, rect.center().y), Align2::RIGHT_CENTER, t, FontId::monospace(12.5), c);
        }
        let sw = egui::Rect::from_center_size(egui::pos2(rect.right() - 27.0, rect.center().y), Vec2::new(34.0, 18.0));
        let enabled = !r.cidrs.is_empty() && (can_block || blocked);
        let tip = if r.cidrs.is_empty() { "Valve publishes no relay IPs here" } else if blocked { "Blocked: click to allow" } else { "Allowed: click to block" };
        let clicked = ui.put(sw, |ui: &mut egui::Ui| th::switch(ui, &r.name, !blocked, false, enabled)).on_hover_text(tip).clicked();
        clicked.then_some(!blocked)
    }

    fn list(&mut self, ui: &mut egui::Ui) -> Option<(Vec<String>, bool)> {
        let tab = self.tab;
        let can_block = self.exes[tab].is_some();
        let p = self.profs[tab].clone();
        let mut out = None;
        let blocked_n = p.regions.iter().filter(|r| self.is_blocked(&p.game, &r.id)).count();
        ui.horizontal(|ui| {
            ui.heading(if p.groups().is_empty() { "Regions" } else { "Locations" });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let txt = if blocked_n == 0 { "all allowed".to_string() } else { format!("{blocked_n} blocked") };
                ui.label(RichText::new(txt).small().color(if blocked_n == 0 { th::FAINT } else { th::BAD }));
            });
        });
        ui.add_space(4.0);
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            let groups = p.groups();
            if groups.is_empty() {
                for r in &p.regions {
                    if let Some(b) = self.region_row(ui, &p, r, can_block, 0.0) {
                        out = Some((vec![r.id.clone()], b));
                    }
                }
            }
            for g in groups {
                let ids: Vec<String> = p.regions.iter().filter(|r| r.group.as_deref() == Some(g) && !r.cidrs.is_empty()).map(|r| r.id.clone()).collect();
                let nb = ids.iter().filter(|id| self.is_blocked(&p.game, id)).count();
                let open = self.expanded.contains(g);
                // group header: click to expand, switch flips the whole area
                let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_H), Sense::click());
                let painter = ui.painter_at(rect);
                if resp.hovered() {
                    painter.rect_filled(rect, CornerRadius::same(th::R), th::HOVER);
                }
                let t = ui.ctx().animate_bool_with_time(resp.id, open, 0.12);
                chevron(&painter, egui::pos2(rect.left() + 14.0, rect.center().y), t, th::DIM);
                painter.text(egui::pos2(rect.left() + 28.0, rect.center().y), Align2::LEFT_CENTER, g, FontId::new(14.0, semibold()), th::TEXT);
                let count = if nb == 0 { format!("{}", ids.len()) } else { format!("{nb}/{} blocked", ids.len()) };
                painter.text(egui::pos2(rect.right() - 58.0, rect.center().y), Align2::RIGHT_CENTER, count, FontId::proportional(12.0), if nb == 0 { th::FAINT } else { th::BAD });
                let sw = egui::Rect::from_center_size(egui::pos2(rect.right() - 27.0, rect.center().y), Vec2::new(34.0, 18.0));
                let mixed = nb > 0 && nb < ids.len();
                let all_blocked = !ids.is_empty() && nb == ids.len();
                let enabled = !ids.is_empty() && (can_block || nb > 0);
                let sr = ui.put(sw, |ui: &mut egui::Ui| th::switch(ui, g, !all_blocked, mixed, enabled))
                    .on_hover_text(if nb > 0 { "Allow every city here" } else { "Block every city here" });
                if sr.clicked() {
                    out = Some((ids.clone(), nb == 0));
                } else if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    if open { self.expanded.remove(g); } else { self.expanded.insert(g.to_string()); }
                }
                if open {
                    for r in p.regions.iter().filter(|r| r.group.as_deref() == Some(g)) {
                        if let Some(b) = self.region_row(ui, &p, r, can_block, 18.0) {
                            out = Some((vec![r.id.clone()], b));
                        }
                    }
                    ui.add_space(4.0);
                }
            }
        });
        out
    }

    fn presets_row(&mut self, ui: &mut egui::Ui) {
        let game = self.profs[self.tab].game.clone();
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("PRESETS").small().color(th::FAINT));
            ui.add_space(4.0);
            let mine: Vec<Preset> = self.presets.iter().filter(|p| p.game == game).cloned().collect();
            if mine.is_empty() && self.naming.is_none() {
                ui.label(RichText::new("save your current blocks to switch back in one click").small().color(th::FAINT));
            }
            for pr in &mine {
                let tip = if pr.blocked.is_empty() { "allows everything".into() } else { format!("blocks {}", pr.blocked.join(", ")) };
                let r = ui.add(egui::Button::new(&pr.name).corner_radius(CornerRadius::same(12)))
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text(format!("{tip}\nright-click to delete"));
                if r.clicked() {
                    self.request_preset(&preset_label(pr));
                }
                r.context_menu(|ui| {
                    if ui.button("Delete preset").clicked() {
                        self.presets.retain(|p| !(p.game == game && p.name == pr.name));
                        self.save_presets();
                        ui.close();
                    }
                });
            }
            match self.naming.take() {
                None => {
                    if th::ghost(ui, RichText::new("+ Save current").color(th::DIM)).clicked() {
                        self.naming = Some(String::new());
                    }
                }
                Some(mut name) => {
                    let te = ui.add(egui::TextEdit::singleline(&mut name).hint_text("preset name").desired_width(140.0));
                    te.request_focus();
                    let enter = te.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    let save = ui.add_enabled(!name.trim().is_empty(), egui::Button::new("Save")).clicked() || (enter && !name.trim().is_empty());
                    let cancel = th::ghost(ui, RichText::new("Cancel").color(th::DIM)).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape));
                    if save {
                        if let Ok(fw) = Fw::open() {
                            let name = name.trim().to_string();
                            let blocked = presets::snapshot(&fw, &self.profs[self.tab]);
                            self.presets.retain(|p| !(p.game == game && p.name == name));
                            self.say(format!("Saved preset \u{201c}{name}\u{201d}"), false);
                            self.presets.push(Preset { name, game: game.clone(), blocked });
                            self.save_presets();
                        }
                    } else if !cancel {
                        self.naming = Some(name);
                    }
                }
            }
        });
    }

    /// Three lowest-ping regions you can currently reach.
    fn fastest_row(&self, ui: &mut egui::Ui) {
        let p = &self.profs[self.tab];
        let mut best: Vec<(&str, u32)> = p.regions.iter()
            .filter(|r| !self.is_blocked(&p.game, &r.id))
            .filter_map(|r| Some((r.name.as_str(), (*self.pings.get(&(p.game.clone(), r.id.clone()))?)?)))
            .collect();
        best.sort_by_key(|b| b.1);
        ui.horizontal(|ui| {
            let est = p.game == "overwatch";
            ui.label(RichText::new(if est { "FASTEST NOW (EST.)" } else { "FASTEST NOW" }).small().color(th::FAINT))
                .on_hover_text(if est { "Estimated from the nearest AWS region, not the game server" } else { "ICMP to each city's Valve relay" });
            ui.add_space(4.0);
            if best.is_empty() {
                ui.label(RichText::new("measuring\u{2026}").small().color(th::FAINT));
            }
            for (name, ms) in best.into_iter().take(3) {
                ui.label(RichText::new(name).color(th::DIM));
                ui.label(RichText::new(format!("{}{ms} ms", if est { "~" } else { "" })).monospace().color(th::ping_color(ms)));
                ui.add_space(10.0);
            }
        });
    }

    fn map(&mut self, ui: &mut egui::Ui) -> Option<(Vec<String>, bool)> {
        let p = &self.profs[self.tab];
        let can_block = self.exes[self.tab].is_some();
        let cur = self.monitor.current();
        let markers: Vec<Marker> = p.regions.iter().filter(|r| r.lat != 0.0 || r.lon != 0.0).map(|r| Marker {
            id: &r.id,
            label: &r.name,
            lat: r.lat,
            lon: r.lon,
            state: if r.cidrs.is_empty() { MarkerState::Unknown }
                   else if self.is_blocked(&p.game, &r.id) { MarkerState::Blocked }
                   else { MarkerState::Allowed },
            ping_ms: self.pings.get(&(p.game.clone(), r.id.clone())).copied().flatten(),
            connected: cur.as_ref().is_some_and(|c| c.game == p.game && c.region == r.id),
        }).collect();
        let view = &mut self.map;
        let hit = egui::Frame::new().stroke(Stroke::new(1.0, th::STROKE)).corner_radius(CornerRadius::same(th::R))
            .show(ui, |ui| view.show(ui, &markers)).inner;
        let mut out = None;
        if let Some(r) = hit.and_then(|id| p.region(&id)).filter(|r| !r.cidrs.is_empty()) {
            let b = self.blocked.get(&(p.game.clone(), r.id.clone())).copied().unwrap_or(false);
            if can_block || b {
                out = Some((vec![r.id.clone()], !b));
            }
        }
        ui.label(RichText::new("click a dot to block or allow it \u{b7} scroll to zoom \u{b7} drag to pan \u{b7} double-click to reset").small().color(th::FAINT));
        out
    }
}

/// Dev only: SVTOGGLE_SHOT=<file.bmp> saves the window's own framebuffer ~3 s after start, then exits.
/// Used for README/preview shots without screen-grabbing whatever else is on the desktop.
fn dev_screenshot(ctx: &egui::Context) {
    use std::sync::atomic::{AtomicBool, Ordering};
    static ASKED: AtomicBool = AtomicBool::new(false);
    let Some(path) = std::env::var_os("SVTOGGLE_SHOT") else { return };
    ctx.request_repaint_after(Duration::from_millis(200));
    if ctx.input(|i| i.time) < 3.0 || ASKED.swap(true, Ordering::Relaxed) {
        return;
    }
    ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::ScreenshotCallback::new(move |img| {
        let [w, h] = img.size;
        let mut bmp = Vec::with_capacity(54 + w * h * 4);
        let le = |v: u32| v.to_le_bytes();
        bmp.extend_from_slice(b"BM");
        bmp.extend_from_slice(&le((54 + w * h * 4) as u32));
        bmp.extend_from_slice(&[0, 0, 0, 0]);
        bmp.extend_from_slice(&le(54));
        bmp.extend_from_slice(&le(40));
        bmp.extend_from_slice(&le(w as u32));
        bmp.extend_from_slice(&(-(h as i32)).to_le_bytes()); // top-down
        bmp.extend_from_slice(&[1, 0, 32, 0]);
        bmp.extend_from_slice(&[0u8; 24]);
        for p in &img.pixels {
            bmp.extend_from_slice(&[p.b(), p.g(), p.r(), 255]);
        }
        let _ = std::fs::write(&path, bmp);
        std::process::exit(0);
    })));
}

fn chevron(p: &egui::Painter, c: egui::Pos2, t: f32, color: Color32) {
    // right-pointing at t=0, down at t=1
    let a = t * std::f32::consts::FRAC_PI_2;
    let rot = |x: f32, y: f32| c + Vec2::new(x * a.cos() - y * a.sin(), x * a.sin() + y * a.cos());
    p.add(egui::Shape::line(vec![rot(-2.0, -4.0), rot(2.0, 0.0), rot(-2.0, 4.0)], Stroke::new(1.5, color)));
}

fn nice_game(g: &str) -> &str {
    match g {
        "overwatch" => "Overwatch",
        "deadlock" => "Deadlock",
        other => other,
    }
}

fn preset_label(p: &Preset) -> String {
    format!("{}: {}", nice_game(&p.game), p.name)
}

fn preset_labels(ps: &[Preset]) -> Vec<String> {
    ps.iter().map(preset_label).collect()
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        dev_screenshot(&ctx);
        if self.tray.is_some() {
            tray::handle_close_to_tray(&ctx);
        }
        for cmd in self.tray.as_ref().map(|t| t.poll()).unwrap_or_default() {
            match cmd {
                TrayCmd::Show => {}
                TrayCmd::AllowAll => self.allow_everything(),
                TrayCmd::Preset(label) => self.request_preset(&label),
                TrayCmd::Quit => std::process::exit(0), // tray thread exits first; kept for completeness
            }
        }
        while let Ok(m) = self.rx.try_recv() {
            match m {
                Msg::Ping(g, id, ms) => {
                    self.pings_out = self.pings_out.saturating_sub(1);
                    self.pings.insert((g, id), ms);
                }
                Msg::Refreshed(i, res) => self.on_refreshed(i, res),
            }
        }
        if self.deferred.is_some() && self.monitor.current().is_none() {
            let (i, old) = self.deferred.take().unwrap();
            self.reapply(i, old, "Match over, blocks updated".into());
        }
        if self.last_status.is_none_or(|t| t.elapsed() >= STATUS_EVERY) {
            self.refresh_status();
        }
        if self.last_ping.is_none_or(|(tab, t)| tab != self.tab || t.elapsed() >= PING_EVERY) {
            self.refresh_pings(&ctx);
        }
        ctx.request_repaint_after(STATUS_EVERY); // idle = one wake per 5 s

        let bar = |m: (i8, i8)| egui::Frame::new().fill(th::PANEL).inner_margin(Margin::symmetric(m.0, m.1));
        egui::Panel::top("top").frame(bar((16, 0))).exact_size(52.0).show(ui, |ui| self.top_bar(ui, &ctx));
        egui::Panel::bottom("status").frame(bar((16, 0))).exact_size(30.0).show(ui, |ui| self.status_bar(ui));

        let mut action = None;
        egui::Panel::right("list")
            .frame(egui::Frame::new().fill(th::BG).inner_margin(Margin { left: 12, right: 14, top: 14, bottom: 8 }))
            .exact_size(LIST_W)
            .resizable(false)
            .show(ui, |ui| {
                self.live_card(ui);
                ui.add_space(10.0);
                action = action.take().or(self.list(ui));
            });
        egui::CentralPanel::no_frame().show(ui, |ui| {
            egui::Frame::new().inner_margin(Margin { left: 16, right: 8, top: 14, bottom: 8 }).show(ui, |ui| {
                // leave room under the map for the hint + presets
                let h = ui.available_height() - 104.0;
                ui.allocate_ui(Vec2::new(ui.available_width(), h.max(200.0)), |ui| {
                    if let Some(a) = self.map(ui) {
                        action = Some(a);
                    }
                });
                ui.add_space(6.0);
                self.presets_row(ui);
                ui.add_space(6.0);
                self.fastest_row(ui);
            });
        });
        if let Some((ids, block)) = action {
            self.request_regions(self.tab, ids, block);
        }
        self.confirm_modal(&ctx);
        if self.about {
            self.about_window(&ctx);
        }
    }
}

fn main() -> eframe::Result {
    // screenshot mode is a throwaway dev run: let it start beside a running copy
    if std::env::var_os("SVTOGGLE_SHOT").is_none() && !svtoggle::single::claim() {
        return Ok(()); // already running: it was told to show its window
    }
    let icon = egui::IconData { rgba: svtoggle::icon::rgba(64), width: 64, height: 64 };
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("svtoggle")
            .with_icon(icon)
            .with_inner_size([1180.0, 560.0])
            .with_min_inner_size([920.0, 520.0]),
        ..Default::default()
    };
    eframe::run_native("svtoggle", opts, Box::new(|cc| Ok(Box::new(App::new(&cc.egui_ctx)))))
}
