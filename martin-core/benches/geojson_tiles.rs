use std::f64::consts::PI;
use std::hint::black_box;
use std::io::Write as _;

use criterion::{Criterion, criterion_group, criterion_main};
use geo_types::{Coord, LineString, Polygon};
use geojson::{Feature, FeatureCollection, GeoJson, Geometry, Value};
use martin_core::CacheZoomRange;
use martin_core::tiles::Source as _;
use martin_core::tiles::geojson::source::GeoJsonSource;
use martin_tile_utils::TileCoord;
use serde_json::{Map, json};
use tokio::runtime::Runtime;

/// Number of polygon features in the synthetic dataset.
/// A perfect square keeps the generated grid even.
const FEATURES: usize = 10_000;
/// Vertices per polygon ring, to exercise the clip/simplify path.
const RING_VERTICES: usize = 16;

/// Write a deterministic synthetic `GeoJSON` `FeatureCollection` to a temp file.
///
/// Features are laid out on a regular grid spanning the whole WGS84 range, each a
/// small `RING_VERTICES`-sided polygon. Generation is fully deterministic (no RNG),
/// so bench numbers are comparable across runs, and the file is removed when the
/// returned handle is dropped, so nothing is committed to the repo.
fn synthetic_geojson() -> tempfile::NamedTempFile {
    let cols = (FEATURES as f64).sqrt().ceil() as usize;
    let rows = cols;
    let lon_step = 360.0 / cols as f64;
    let lat_step = 170.0 / rows as f64;
    let radius = 0.4 * lon_step.min(lat_step);

    let mut features = Vec::with_capacity(rows * cols);
    for r in 0..rows {
        for c in 0..cols {
            let cx = -180.0 + lon_step * (c as f64 + 0.5);
            let cy = -85.0 + lat_step * (r as f64 + 0.5);
            // `Polygon::new` closes the ring for us, so the points need not repeat the first.
            let exterior: LineString = (0..RING_VERTICES)
                .map(|k| {
                    let theta = 2.0 * PI * k as f64 / RING_VERTICES as f64;
                    Coord {
                        x: cx + radius * theta.cos(),
                        y: cy + radius * theta.sin(),
                    }
                })
                .collect();
            let geometry = geo_types::Geometry::from(Polygon::new(exterior, vec![]));

            let mut properties = Map::new();
            properties.insert("id".to_string(), json!(r * cols + c));
            features.push(Feature {
                bbox: None,
                geometry: Some(Geometry::new(Value::from(&geometry))),
                id: None,
                properties: Some(properties),
                foreign_members: None,
            });
        }
    }
    let fc = GeoJson::FeatureCollection(FeatureCollection {
        bbox: None,
        features,
        foreign_members: None,
    });

    let mut tmp = tempfile::Builder::new()
        .suffix(".geojson")
        .tempfile()
        .unwrap();
    serde_json::to_writer(&mut tmp, &fc).unwrap();
    tmp.flush().unwrap();
    tmp
}

fn bench_geojson(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    // The handle must outlive both benches: dropping it deletes the backing file.
    let file = synthetic_geojson();
    let path = file.path().to_path_buf();

    // Load path: read + parse + build the R-tree.
    c.bench_function("build_source", |b| {
        b.to_async(&rt).iter(|| async {
            let source =
                GeoJsonSource::new("bench".to_string(), path.clone(), CacheZoomRange::default())
                    .await
                    .unwrap();
            black_box(source);
        });
    });

    // Fetch path: source built once, so only `get_tile` (search + clip + transform + encode) is timed.
    let source = rt
        .block_on(GeoJsonSource::new(
            "bench".to_string(),
            path.clone(),
            CacheZoomRange::default(),
        ))
        .unwrap();
    c.bench_function("fetch_tile", |b| {
        b.to_async(&rt).iter(|| async {
            let tile = source
                .get_tile(TileCoord::new_unchecked(1, 1, 0), None)
                .await
                .unwrap();
            black_box(tile);
        });
    });
}

criterion_group!(benches, bench_geojson);
criterion_main!(benches);
