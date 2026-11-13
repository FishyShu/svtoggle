//! Builds `app/assets/land.bin` from Natural Earth land GeoJSON (public domain).
//!
//! cargo run -p svtoggle --example gen_land -- ne_110m_land.geojson app/assets/land.bin
//!
//! Format (little-endian), decoded by `svtoggle::map`:
//!   b"LND1", u32 vertex count, u32 index count, u32 ring count,
//!   vertices: (i16 lon, i16 lat) quantized as deg / 180 (lon) or / 90 (lat) * 32767,
//!   indices: u16 triangle list,
//!   rings: u16 vertex count per ring; rings are stored back to back in the vertex array.

use serde_json::Value;

/// Polygons entirely south of this are below the map crop (Antarctica), so they are dropped.
const MIN_LAT: f64 = -60.0;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [input, output] = args.as_slice() else {
        panic!("usage: gen_land <in.geojson> <out.bin>");
    };
    let json: Value = serde_json::from_str(&std::fs::read_to_string(input).unwrap()).unwrap();

    let (mut verts, mut idx, mut rings) =
        (Vec::<[i16; 2]>::new(), Vec::<u16>::new(), Vec::<u16>::new());
    for f in json["features"].as_array().expect("FeatureCollection") {
        let g = &f["geometry"];
        let polys: Vec<&Value> = match g["type"].as_str() {
            Some("Polygon") => vec![&g["coordinates"]],
            Some("MultiPolygon") => g["coordinates"].as_array().unwrap().iter().collect(),
            t => panic!("unexpected geometry {t:?}"),
        };
        for poly in polys {
            let mut flat = Vec::<f64>::new();
            let mut holes = Vec::new();
            let mut lens = Vec::new();
            for (i, ring) in poly.as_array().unwrap().iter().enumerate() {
                let mut pts: Vec<[f64; 2]> = ring
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|p| [p[0].as_f64().unwrap(), p[1].as_f64().unwrap()])
                    .collect();
                if pts.len() > 1 && pts.first() == pts.last() {
                    pts.pop(); // GeoJSON rings repeat the first point
                }
                if i > 0 {
                    holes.push(flat.len() / 2);
                }
                lens.push(pts.len() as u16);
                flat.extend(pts.iter().flatten());
            }
            if flat.chunks(2).all(|p| p[1] < MIN_LAT) {
                continue;
            }
            let tris = earcutr::earcut(&flat, &holes, 2).expect("earcut");
            let base = verts.len();
            for p in flat.chunks(2) {
                verts.push([
                    (p[0] / 180.0 * 32767.0).round() as i16,
                    (p[1] / 90.0 * 32767.0).round() as i16,
                ]);
            }
            idx.extend(
                tris.iter()
                    .map(|&t| u16::try_from(base + t).expect("too many vertices for u16")),
            );
            rings.extend(lens);
        }
    }

    let mut out = b"LND1".to_vec();
    for n in [verts.len(), idx.len(), rings.len()] {
        out.extend((n as u32).to_le_bytes());
    }
    out.extend(verts.iter().flatten().flat_map(|v| v.to_le_bytes()));
    out.extend(idx.iter().flat_map(|v| v.to_le_bytes()));
    out.extend(rings.iter().flat_map(|v| v.to_le_bytes()));
    std::fs::write(output, &out).unwrap();
    println!(
        "{} vertices, {} triangles, {} rings, {} bytes",
        verts.len(),
        idx.len() / 3,
        rings.len(),
        out.len()
    );
}
