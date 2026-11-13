//! Flat dark world map (equirectangular) with clickable server markers.
//!
//! World space is degrees: x = lon + 180 (0..360), y = 90 - lat (0..180), so one uniform
//! scale maps it to screen. Land comes from `assets/land.bin` (see `examples/gen_land.rs`).

use eframe::egui::{
    self, CursorIcon, FontId, Mesh, Pos2, Rect, Sense, Shape, Stroke, Vec2,
    emath::TSTransform,
};
use std::sync::OnceLock;

const LAT_MAX: f32 = 80.0;
const LAT_MIN: f32 = -58.0;
const TOP: f32 = 90.0 - LAT_MAX;
const BOTTOM: f32 = 90.0 - LAT_MIN;
const ASPECT: f32 = 360.0 / (BOTTOM - TOP);
const MAX_ZOOM: f32 = 16.0;
/// Below this zoom only the hovered marker gets a label (Europe is a dozen dots in a thumbnail).
const LABEL_ZOOM: f32 = 2.5;
const HIT_RADIUS: f32 = 10.0;

use crate::theme::{ACCENT, BAD as RED, COAST, DIM as GREY, LAND, OK as GREEN, TEXT as LABEL, WATER};

pub struct Marker<'a> {
    pub id: &'a str,
    pub label: &'a str,
    pub lat: f32,
    pub lon: f32,
    pub state: MarkerState,
    pub ping_ms: Option<u32>,
    pub connected: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarkerState {
    Allowed,
    Blocked,
    Unknown,
}

/// Zoom + pan state. `Default` shows the whole (cropped) world.
#[derive(Clone, Debug)]
pub struct MapView {
    zoom: f32,
    center: Pos2, // world space
}

impl Default for MapView {
    fn default() -> Self {
        MapView {
            zoom: 1.0,
            center: Pos2::new(180.0, (TOP + BOTTOM) / 2.0),
        }
    }
}

pub fn project(lat: f32, lon: f32) -> Pos2 {
    Pos2::new(lon + 180.0, 90.0 - lat)
}

pub fn unproject(p: Pos2) -> (f32, f32) {
    (90.0 - p.y, p.x - 180.0)
}

impl MapView {
    /// Screen pixels per degree.
    fn ppd(&self, rect: Rect) -> f32 {
        rect.width() * self.zoom / 360.0
    }

    fn to_screen(&self, rect: Rect, p: Pos2) -> Pos2 {
        rect.center() + (p - self.center) * self.ppd(rect)
    }

    fn from_screen(&self, rect: Rect, s: Pos2) -> Pos2 {
        self.center + (s - rect.center()) / self.ppd(rect)
    }

    /// Keeps the visible window inside the cropped world.
    fn clamp(&mut self) {
        self.zoom = self.zoom.clamp(1.0, MAX_ZOOM);
        let (hw, hh) = (180.0 / self.zoom, (BOTTOM - TOP) / 2.0 / self.zoom);
        self.center.x = self.center.x.clamp(hw, 360.0 - hw);
        self.center.y = self.center.y.clamp(TOP + hh, BOTTOM - hh);
    }

    /// Draws the map at full available width. Returns the id of a clicked marker.
    pub fn show(&mut self, ui: &mut egui::Ui, markers: &[Marker]) -> Option<String> {
        let w = ui.available_width().min(ui.available_height() * ASPECT);
        let (rect, mut response) =
            ui.allocate_exact_size(Vec2::new(w, w / ASPECT), Sense::click_and_drag());

        // Input.
        if response.double_clicked() {
            *self = MapView::default();
        }
        if response.dragged() {
            self.center -= response.drag_delta() / self.ppd(rect);
            ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
        }
        if let Some(cursor) = response.hover_pos() {
            let (scroll, pinch) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
            let factor = pinch * (scroll / 300.0).exp();
            if factor != 1.0 {
                let anchor = self.from_screen(rect, cursor);
                self.zoom = (self.zoom * factor).clamp(1.0, MAX_ZOOM);
                self.center = anchor - (cursor - rect.center()) / self.ppd(rect);
                ui.ctx().input_mut(|i| i.smooth_scroll_delta = Vec2::ZERO); // don't also scroll a parent
            }
        }
        self.clamp();

        // Land.
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, WATER);
        let ppd = self.ppd(rect);
        let land = land();
        let mut mesh = land.mesh.clone();
        mesh.transform(TSTransform::new(
            rect.center().to_vec2() - self.center.to_vec2() * ppd,
            ppd,
        ));
        painter.add(Shape::mesh(mesh));
        for ring in &land.rings {
            let pts = ring.iter().map(|&p| self.to_screen(rect, p)).collect();
            painter.add(Shape::closed_line(pts, Stroke::new(1.0, COAST)));
        }

        // Markers.
        let pos: Vec<Pos2> = markers
            .iter()
            .map(|m| self.to_screen(rect, project(m.lat, m.lon)))
            .collect();
        let hovered = response.hover_pos().and_then(|c| {
            (0..markers.len())
                .filter(|&i| pos[i].distance(c) <= HIT_RADIUS)
                .min_by(|&a, &b| pos[a].distance(c).total_cmp(&pos[b].distance(c)))
        });
        let time = ui.input(|i| i.time);
        let mut pulsing = false;
        for (i, m) in markers.iter().enumerate() {
            let p = pos[i];
            if !rect.expand(HIT_RADIUS).contains(p) {
                continue;
            }
            let color = match m.state {
                MarkerState::Allowed => GREEN,
                MarkerState::Blocked => RED,
                MarkerState::Unknown => GREY,
            };
            if m.connected {
                pulsing = true;
                let t = (time % 1.4) as f32 / 1.4;
                painter.circle_stroke(
                    p,
                    6.0 + 10.0 * t,
                    Stroke::new(1.5, ACCENT.gamma_multiply(1.0 - t)),
                );
                painter.circle_stroke(p, 7.0, Stroke::new(1.5, ACCENT));
            }
            let r = if hovered == Some(i) { 6.0 } else { 4.0 };
            painter.circle_filled(p, r, color);
            painter.circle_stroke(p, r, Stroke::new(1.0, WATER));
        }

        // Labels: hovered first so it always wins, then greedily skip anything overlapping.
        let font = FontId::proportional(11.0);
        let mut placed: Vec<Rect> = Vec::new();
        let order = hovered
            .into_iter()
            .chain((0..markers.len()).filter(|&i| Some(i) != hovered && self.zoom >= LABEL_ZOOM));
        for i in order {
            if !rect.contains(pos[i]) {
                continue;
            }
            let text = match markers[i].ping_ms {
                Some(ms) if hovered == Some(i) => format!("{}  {ms} ms", markers[i].label),
                _ => markers[i].label.to_owned(),
            };
            let galley = painter.layout_no_wrap(text, font.clone(), LABEL);
            let at = pos[i] + Vec2::new(8.0, -galley.size().y / 2.0);
            let r = Rect::from_min_size(at, galley.size()).expand(2.0);
            if placed.iter().any(|q| q.intersects(r)) {
                continue;
            }
            placed.push(r);
            painter.rect_filled(r, 4.0, crate::theme::PANEL.gamma_multiply(0.92));
            painter.galley(at, galley, LABEL);
        }

        // Only a connected marker animates; otherwise the map is fully event-driven.
        if pulsing {
            // 30 fps is plenty for a pulse; full refresh rate would steal frames from the game
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(33));
        }

        let i = hovered?;
        let m = &markers[i];
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        let ping = m
            .ping_ms
            .map_or("no ping".to_owned(), |ms| format!("{ms} ms"));
        response = response.on_hover_text_at_pointer(format!("{}\n{ping}", m.label));
        response.clicked().then(|| m.id.to_owned())
    }
}

struct Land {
    mesh: Mesh,
    rings: Vec<Vec<Pos2>>,
}

fn land() -> &'static Land {
    static LAND_MESH: OnceLock<Land> = OnceLock::new();
    LAND_MESH.get_or_init(|| decode(include_bytes!("../assets/land.bin")))
}

/// Decodes the format written by `examples/gen_land.rs`. The asset is compiled in and
/// checked by tests, so a malformed file is a build bug and panics.
fn decode(b: &[u8]) -> Land {
    assert_eq!(&b[..4], b"LND1", "bad land.bin magic");
    let count = |o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap()) as usize;
    let (nv, ni, nr) = (count(4), count(8), count(12));
    let mut words = b[16..].chunks_exact(2).map(|c| [c[0], c[1]]);
    let mut next_i16 = || i16::from_le_bytes(words.next().expect("land.bin truncated"));

    let pts: Vec<Pos2> = (0..nv)
        .map(|_| {
            let lon = next_i16() as f32 * 180.0 / 32767.0;
            let lat = next_i16() as f32 * 90.0 / 32767.0;
            project(lat, lon)
        })
        .collect();
    let mut mesh = Mesh::default();
    for &p in &pts {
        mesh.colored_vertex(p, LAND);
    }
    mesh.indices = (0..ni).map(|_| next_i16() as u16 as u32).collect();
    let mut start = 0;
    let rings = (0..nr)
        .map(|_| {
            let n = next_i16() as u16 as usize;
            start += n;
            pts[start - n..start].to_vec()
        })
        .collect();
    Land { mesh, rings }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn land_decodes() {
        let l = land();
        let nv = l.mesh.vertices.len();
        assert!((1000..65536).contains(&nv), "{nv} vertices");
        assert!(l.mesh.indices.len() >= 3 * 1000 && l.mesh.indices.len() % 3 == 0);
        assert!(l.mesh.indices.iter().all(|&i| (i as usize) < nv));
        assert_eq!(l.rings.iter().map(Vec::len).sum::<usize>(), nv);
        assert!(l.rings.len() > 50);
        assert!(
            l.mesh
                .vertices
                .iter()
                .all(|v| (0.0..=360.0).contains(&v.pos.x) && (0.0..=180.0).contains(&v.pos.y))
        );
    }

    #[test]
    fn projection_round_trips() {
        let mut v = MapView {
            zoom: 3.7,
            center: Pos2::new(200.0, 50.0),
        };
        v.clamp();
        let rect = Rect::from_min_size(Pos2::new(10.0, 20.0), Vec2::new(800.0, 800.0 / ASPECT));
        for (lat, lon) in [
            (32.08, 34.78),
            (51.5, -0.12),
            (-33.9, 151.2),
            (0.0, 0.0),
            (79.0, -179.0),
        ] {
            let s = v.to_screen(rect, project(lat, lon));
            let (lat2, lon2) = unproject(v.from_screen(rect, s));
            assert!(
                (lat - lat2).abs() < 1e-3 && (lon - lon2).abs() < 1e-3,
                "{lat},{lon} -> {lat2},{lon2}"
            );
        }
        // Default view shows exactly the cropped world.
        let d = MapView::default();
        let tl = d.to_screen(rect, project(LAT_MAX, -180.0));
        assert!(tl.distance(rect.min) < 1e-3);
    }

    #[test]
    fn clamp_keeps_view_inside_world() {
        let mut v = MapView {
            zoom: 99.0,
            center: Pos2::new(-50.0, 500.0),
        };
        v.clamp();
        assert_eq!(v.zoom, MAX_ZOOM);
        assert!(v.center.x >= 180.0 / MAX_ZOOM && v.center.y <= BOTTOM);
    }
}
