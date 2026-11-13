//! Map widget playground with fake markers: `cargo run -p svtoggle --example map_demo`.

use svtoggle::map::{MapView, Marker, MarkerState};

fn main() -> eframe::Result {
    let mut view = MapView::default();
    let mut last = String::new();
    eframe::run_ui_native("map demo", Default::default(), move |ui, _| {
        let m = |id, label, lat, lon, state, ping_ms, connected| Marker {
            id,
            label,
            lat,
            lon,
            state,
            ping_ms,
            connected,
        };
        let markers = [
            m(
                "fra",
                "Frankfurt",
                50.1,
                8.7,
                MarkerState::Allowed,
                Some(48),
                true,
            ),
            m(
                "ams",
                "Amsterdam",
                52.4,
                4.9,
                MarkerState::Allowed,
                Some(55),
                false,
            ),
            m(
                "par",
                "Paris",
                48.9,
                2.35,
                MarkerState::Blocked,
                Some(60),
                false,
            ),
            m(
                "lhr",
                "London",
                51.5,
                -0.12,
                MarkerState::Blocked,
                None,
                false,
            ),
            m(
                "mad",
                "Madrid",
                40.42,
                -3.70,
                MarkerState::Allowed,
                Some(40),
                false,
            ),
            m(
                "bah",
                "Bahrain",
                26.07,
                50.56,
                MarkerState::Blocked,
                Some(70),
                false,
            ),
            m(
                "iad",
                "Virginia",
                38.9,
                -77.4,
                MarkerState::Unknown,
                None,
                false,
            ),
            m(
                "syd",
                "Sydney",
                -33.9,
                151.2,
                MarkerState::Unknown,
                Some(300),
                false,
            ),
        ];
        if let Some(id) = view.show(ui, &markers) {
            last = id;
        }
        ui.label(format!("clicked: {last}"));
    })
}
