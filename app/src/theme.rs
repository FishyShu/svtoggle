//! One look for the whole app: graphite surfaces, IBM Plex, a single amber accent that only ever
//! means "live", muted green/red for allowed/blocked. Every color in the UI comes from here.
use eframe::egui::{self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Stroke, TextStyle, Vec2};
use std::sync::Arc;

pub const BG: Color32 = Color32::from_rgb(14, 17, 20);
pub const PANEL: Color32 = Color32::from_rgb(20, 24, 29);
pub const RAISED: Color32 = Color32::from_rgb(27, 32, 39);
pub const HOVER: Color32 = Color32::from_rgb(33, 39, 47);
pub const STROKE: Color32 = Color32::from_rgb(38, 45, 54);
pub const TEXT: Color32 = Color32::from_rgb(217, 222, 228);
pub const DIM: Color32 = Color32::from_rgb(139, 149, 161);
pub const FAINT: Color32 = Color32::from_rgb(126, 136, 148); // ~5:1 on BG/PANEL: readable small print, still quieter than DIM
pub const ACCENT: Color32 = Color32::from_rgb(232, 163, 61); // amber: live / connected, nothing else
pub const OK: Color32 = Color32::from_rgb(108, 191, 132);
pub const BAD: Color32 = Color32::from_rgb(217, 100, 89);
pub const MID: Color32 = Color32::from_rgb(201, 180, 88);

pub const WATER: Color32 = Color32::from_rgb(15, 19, 24);
pub const LAND: Color32 = Color32::from_rgb(30, 36, 44);
pub const COAST: Color32 = Color32::from_rgb(44, 53, 64);

pub const R: u8 = 6; // corner radius everywhere

/// Ping text color: good / okay / bad. Thresholds are for picking a server, not for perfection.
pub fn ping_color(ms: u32) -> Color32 {
    match ms {
        0..=79 => OK,
        80..=149 => MID,
        _ => BAD,
    }
}

pub fn semibold() -> FontFamily {
    FontFamily::Name("semibold".into())
}

pub fn install(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let add = |f: &mut FontDefinitions, name: &str, bytes: &'static [u8]| {
        f.font_data.insert(name.into(), Arc::new(FontData::from_static(bytes)));
    };
    add(&mut fonts, "plex", include_bytes!("../assets/fonts/IBMPlexSans-Regular.ttf"));
    add(&mut fonts, "plex-semibold", include_bytes!("../assets/fonts/IBMPlexSans-SemiBold.ttf"));
    add(&mut fonts, "plex-mono", include_bytes!("../assets/fonts/IBMPlexMono-Regular.ttf"));
    // Plex first, egui's defaults stay behind it as glyph fallback
    fonts.families.get_mut(&FontFamily::Proportional).unwrap().insert(0, "plex".into());
    fonts.families.get_mut(&FontFamily::Monospace).unwrap().insert(0, "plex-mono".into());
    let mut sb = vec!["plex-semibold".to_string()];
    sb.extend(fonts.families[&FontFamily::Proportional].clone());
    fonts.families.insert(semibold(), sb);
    ctx.set_fonts(fonts);

    ctx.all_styles_mut(|style| {
        style.text_styles = [
            (TextStyle::Small, FontId::proportional(11.5)),
            (TextStyle::Body, FontId::proportional(14.0)),
            (TextStyle::Button, FontId::proportional(13.5)),
            (TextStyle::Heading, FontId::new(17.0, semibold())),
            (TextStyle::Monospace, FontId::monospace(13.0)),
        ]
        .into();
        let s = &mut style.spacing;
        s.item_spacing = Vec2::new(8.0, 6.0);
        s.button_padding = Vec2::new(12.0, 5.0);
        s.window_margin = Margin::same(12);
        s.interact_size.y = 28.0;
        s.scroll.bar_width = 6.0;
        s.scroll.floating = true;

        let v = &mut style.visuals;
        *v = egui::Visuals::dark();
        v.panel_fill = BG;
        v.window_fill = PANEL;
        v.extreme_bg_color = BG;
        v.faint_bg_color = PANEL;
        v.override_text_color = Some(TEXT);
        v.hyperlink_color = ACCENT;
        v.selection.bg_fill = HOVER;
        v.selection.stroke = Stroke::new(1.0, TEXT);
        v.window_corner_radius = CornerRadius::same(R);
        v.window_stroke = Stroke::new(1.0, STROKE);
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, STROKE);
        v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, DIM);
        for (w, fill, stroke) in [
            (&mut v.widgets.inactive, RAISED, STROKE),
            (&mut v.widgets.hovered, HOVER, Color32::from_rgb(58, 67, 79)),
            (&mut v.widgets.active, Color32::from_rgb(40, 47, 56), Color32::from_rgb(70, 80, 93)),
        ] {
            w.bg_fill = fill;
            w.weak_bg_fill = fill;
            w.bg_stroke = Stroke::new(1.0, stroke);
            w.fg_stroke = Stroke::new(1.0, TEXT);
            w.corner_radius = CornerRadius::same(R);
            w.expansion = 0.0; // no grow-on-hover wobble
        }
        v.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT);
    });
}

/// Animated on/off switch. `on` = traffic allowed (green), off = blocked (red). Returns true when clicked.
/// `mixed` draws a half-way knob for a group where only some cities are blocked.
/// `label` names it for screen readers (accesskit) and it's keyboard-focusable (Tab, then Space/Enter).
pub fn switch(ui: &mut egui::Ui, label: &str, on: bool, mixed: bool, enabled: bool) -> egui::Response {
    let size = Vec2::new(34.0, 18.0);
    let sense = if enabled { egui::Sense::click() } else { egui::Sense::hover() };
    let (rect, resp) = ui.allocate_exact_size(size, sense);
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, enabled, on && !mixed, format!("{label}: allowed")));
    if resp.has_focus() {
        ui.painter().rect_stroke(rect.expand(3.0), CornerRadius::same(11), Stroke::new(1.5, TEXT), egui::StrokeKind::Outside);
    }
    let t = ui.ctx().animate_bool_with_time(resp.id, on, 0.14);
    let t = if mixed { 0.5 } else { t };
    if ui.is_rect_visible(rect) {
        let track = if mixed { MID } else { lerp(BAD, OK, t) };
        let track = if enabled { track } else { track.gamma_multiply(0.35) };
        let track = if resp.hovered() { track } else { track.gamma_multiply(0.85) };
        let p = ui.painter();
        p.rect_filled(rect, CornerRadius::same(9), track.gamma_multiply(0.30));
        p.rect_stroke(rect, CornerRadius::same(9), Stroke::new(1.0, track.gamma_multiply(0.7)), egui::StrokeKind::Inside);
        let x = egui::lerp((rect.left() + 9.0)..=(rect.right() - 9.0), t);
        p.circle_filled(egui::pos2(x, rect.center().y), 6.0, if enabled { TEXT } else { DIM });
    }
    if enabled {
        resp.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        resp
    }
}

fn lerp(a: Color32, b: Color32, t: f32) -> Color32 {
    let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(f(a.r(), b.r()), f(a.g(), b.g()), f(a.b(), b.b()))
}

/// Flat text button that only shows a surface on hover (toolbar actions, chips).
pub fn ghost(ui: &mut egui::Ui, text: impl Into<egui::WidgetText>) -> egui::Response {
    ui.add(egui::Button::new(text).fill(Color32::TRANSPARENT).stroke(Stroke::NONE))
        .on_hover_cursor(egui::CursorIcon::PointingHand)
}
