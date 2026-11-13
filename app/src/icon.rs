// The svtoggle mark, drawn in code (no image files): a graphite tile, a light globe outline,
// and an amber "live" dot. Shared by build.rs (exe .ico), the window icon and the tray.
// Pure std on purpose: build.rs `include!`s this file.

/// RGBA pixels, `size` x `size`, anti-aliased by 4x4 supersampling.
pub fn rgba(size: u32) -> Vec<u8> {
    let s = size as f32;
    let mut out = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let mut acc = [0f32; 4];
            for sy in 0..4 {
                for sx in 0..4 {
                    // sample in 0..1 tile space
                    let u = (x as f32 + (sx as f32 + 0.5) / 4.0) / s;
                    let v = (y as f32 + (sy as f32 + 0.5) / 4.0) / s;
                    let c = sample(u, v, s);
                    // premultiplied accumulate
                    for i in 0..3 {
                        acc[i] += c[i] * c[3];
                    }
                    acc[3] += c[3];
                }
            }
            let a = acc[3] / 16.0;
            let px = |i: usize| if acc[3] > 0.0 { (acc[i] / acc[3] * 255.0).round() as u8 } else { 0 };
            out.extend_from_slice(&[px(0), px(1), px(2), (a * 255.0).round() as u8]);
        }
    }
    out
}

fn sample(u: f32, v: f32, s: f32) -> [f32; 4] {
    const TILE: [f32; 3] = [0.102, 0.122, 0.149]; // #1A1F26
    const LINE: [f32; 3] = [0.80, 0.83, 0.87];
    const AMBER: [f32; 3] = [0.91, 0.64, 0.24]; // #E8A33D
    // rounded-square tile
    let r = 0.22;
    let (dx, dy) = ((u - 0.5).abs() - (0.5 - r), (v - 0.5).abs() - (0.5 - r));
    let outside = (dx.max(0.0).powi(2) + dy.max(0.0).powi(2)).sqrt() - r;
    if outside > 0.0 {
        return [0.0; 4];
    }
    // lines get thicker in px terms at small sizes so the mark survives 16x16
    let w = (1.6 / s).max(0.045);
    let (cx, cy) = (u - 0.47, v - 0.53);
    let rad = 0.30;
    let d = (cx * cx + cy * cy).sqrt();
    let ring = (d - rad).abs() < w;
    let inside = d < rad;
    let detail = s >= 24.0; // equator + meridian turn to mush at 16-20 px
    let equator = detail && inside && cy.abs() < w * 0.7;
    // one meridian ellipse: x²/(0.42r)² + y²/r² = 1
    let e = ((cx / (rad * 0.42)).powi(2) + (cy / rad).powi(2)).sqrt();
    let meridian = detail && inside && (e - 1.0).abs() < w / (rad * 0.42) * 0.7;
    // amber dot, upper right, with a tile-colored gap ring so it reads on top of the globe
    let (ax, ay) = (u - 0.73, v - 0.27);
    let ad = (ax * ax + ay * ay).sqrt();
    if ad < 0.13 {
        return [AMBER[0], AMBER[1], AMBER[2], 1.0];
    }
    if ad < 0.13 + w * 1.4 {
        return [TILE[0], TILE[1], TILE[2], 1.0];
    }
    let c = if ring || equator || meridian { LINE } else { TILE };
    [c[0], c[1], c[2], 1.0]
}

/// Minimal .ico (32-bit BMP entries) for build.rs.
pub fn ico(sizes: &[u32]) -> Vec<u8> {
    let imgs: Vec<Vec<u8>> = sizes.iter().map(|&n| bmp(n)).collect();
    let mut out = vec![0, 0, 1, 0];
    out.extend_from_slice(&(sizes.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * sizes.len() as u32;
    for (n, img) in sizes.iter().zip(&imgs) {
        let b = if *n >= 256 { 0 } else { *n as u8 }; // 0 means 256 in ICONDIRENTRY
        out.extend_from_slice(&[b, b, 0, 0, 1, 0, 32, 0]);
        out.extend_from_slice(&(img.len() as u32).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset += img.len() as u32;
    }
    for img in imgs {
        out.extend(img);
    }
    out
}

fn bmp(n: u32) -> Vec<u8> {
    let px = rgba(n);
    let mask_row = n.div_ceil(32) * 4;
    let mut out = Vec::new();
    for v in [40u32, n, n * 2] {
        out.extend_from_slice(&v.to_le_bytes()); // header size, width, height (x2: color + mask)
    }
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(&[0u8; 24]); // compression .. important colors
    for y in (0..n).rev() {
        for x in 0..n {
            let i = ((y * n + x) * 4) as usize;
            out.extend_from_slice(&[px[i + 2], px[i + 1], px[i], px[i + 3]]); // BGRA, bottom-up
        }
    }
    out.extend(std::iter::repeat_n(0u8, (mask_row * n) as usize)); // AND mask unused with alpha
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn icon_shapes() {
        let p = super::rgba(32);
        assert_eq!(p.len(), 32 * 32 * 4);
        assert_eq!(p[3], 0, "corner is transparent (rounded tile)");
        assert_eq!(p[(16 * 32 + 16) * 4 + 3], 255, "center is opaque");
        let ico = super::ico(&[16, 32]);
        assert_eq!(&ico[..6], &[0, 0, 1, 0, 2, 0]);
    }
}
