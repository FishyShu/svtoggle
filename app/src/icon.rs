// The svtoggle mascot, drawn in code (no image files, no AI art): a chubby amber fish blowing bubbles on a
// graphite tile. Shared by build.rs (exe .ico), the window icon, the tray and the docs PNGs.
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
    const AMBER: [f32; 3] = [0.91, 0.64, 0.24]; // #E8A33D, the app accent
    const FIN: [f32; 3] = [0.80, 0.50, 0.15];
    const BLUSH: [f32; 3] = [0.96, 0.52, 0.47];
    const EYE: [f32; 3] = [0.09, 0.10, 0.12];
    const WHITE: [f32; 3] = [1.0, 1.0, 1.0];
    let px = |c: [f32; 3]| [c[0], c[1], c[2], 1.0];

    // rounded-square tile
    let r = 0.22;
    let (dx, dy) = ((u - 0.5).abs() - (0.5 - r), (v - 0.5).abs() - (0.5 - r));
    if (dx.max(0.0).powi(2) + dy.max(0.0).powi(2)).sqrt() - r > 0.0 {
        return [0.0; 4];
    }
    let detail = s >= 24.0; // at 16-20 px only body, tail and eye survive
    let w = (1.6 / s).max(0.03); // line width, thicker in px terms when small

    // fish space: centred, tilted nose-up so it looks like it's happily swimming up-right
    let (a_sin, a_cos) = (-0.22f32).sin_cos();
    let (fx, fy) = (u - 0.535, v - 0.55);
    let (x, y) = (fx * a_cos + fy * a_sin, -fx * a_sin + fy * a_cos);
    let ell = |cx: f32, cy: f32, rx: f32, ry: f32| ((x - cx) / rx).powi(2) + ((y - cy) / ry).powi(2);
    let circ = |cx: f32, cy: f32| ((x - cx).powi(2) + (y - cy).powi(2)).sqrt();

    // front to back: first hit wins
    if circ(0.115, -0.045) < 0.072 {
        // eye, with a highlight dot: the whole cuteness budget lives here
        return px(if circ(0.135, -0.072) < 0.026 { WHITE } else { EYE });
    }
    let body = ell(0.0, 0.0, 0.27, 0.215);
    if body < 1.0 {
        if detail && circ(0.235, 0.035) < 0.04 && circ(0.235, 0.035) > 0.04 - w * 0.9 && y > 0.035 {
            return px(EYE); // tiny smile
        }
        if detail && ell(0.115, 0.075, 0.05, 0.028) < 1.0 {
            return px(BLUSH);
        }
        return px(AMBER);
    }
    // fan tail: a wedge that widens away from the body, with a round notch bitten out = the fork
    let t = -0.2 - x; // 0 at the body, grows toward the tail tip
    if (0.0..0.18).contains(&t) && y.abs() < 0.05 + t * 0.95 && circ(-0.405, 0.0) > 0.075 {
        return px(FIN);
    }
    // little dorsal fin, leaning back
    if y < -0.17 && ell(-0.06, -0.19, 0.075, 0.06) < 1.0 {
        return px(FIN);
    }
    if detail {
        // bubbles up by the mouth
        for (bx, by, br) in [(0.80, 0.24, 0.05), (0.87, 0.12, 0.032)] {
            let d = ((u - bx).powi(2) + (v - by).powi(2)).sqrt();
            if (d - br).abs() < (w * 0.45).max(0.012) {
                return px(LINE);
            }
        }
    }
    px(TILE)
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
