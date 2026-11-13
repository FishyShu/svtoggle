//! Writes the app mark as raw RGBA files (`icon-<size>.rgba`) for turning into PNGs for docs/README.
//! `cargo run -p svtoggle --example export_icon -- <out_dir>`
fn main() {
    let dir = std::env::args().nth(1).unwrap_or_else(|| ".".into());
    for n in [32u32, 180, 256, 512] {
        std::fs::write(format!("{dir}/icon-{n}.rgba"), svtoggle::icon::rgba(n)).expect("write rgba");
    }
}
