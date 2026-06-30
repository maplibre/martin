use std::f64::consts::PI;
use std::hint::black_box;
use std::io::Write as _;

use criterion::{Criterion, criterion_group, criterion_main};
use geo_types::{Coord, LineString, Polygon};
use geojson::{Feature, FeatureCollection, GeoJson, Geometry, GeometryValue};
use martin_core::CacheZoomRange;
use martin_core::tiles::Source as _;
use martin_core::tiles::geojson::source::GeoJsonSource;
use martin_tile_utils::TileCoord;
use tokio::runtime::Runtime;

/// Number of polygon features in the synthetic dataset.
/// A perfect square keeps the generated grid even.
const FEATURES: u32 = 10_000;
/// Vertices per polygon ring, to exercise the clip/simplify path.
const RING_VERTICES: u32 = 16;

/// Write a deterministic synthetic `GeoJSON` `FeatureCollection` to a temp file.
///
/// Features are laid out on a regular grid spanning the whole WGS84 range, each a
/// small `RING_VERTICES`-sided polygon. Generation is fully deterministic (no RNG),
/// so bench numbers are comparable across runs, and the file is removed when the
/// returned handle is dropped, so nothing is committed to the repo.
fn synthetic_geojson() -> tempfile::NamedTempFile {
    // `FEATURES` is a perfect square, so the integer square root is exact.
    let cols = FEATURES.isqrt();
    let rows = cols;
    let lon_step = 360.0 / f64::from(cols);
    let lat_step = 170.0 / f64::from(rows);
    let radius = 0.4 * lon_step.min(lat_step);

    let features: FeatureCollection = (0..rows * cols)
        .map(|i| {
            let cx = -180.0 + lon_step * (f64::from(i % cols) + 0.5);
            let cy = -85.0 + lat_step * (f64::from(i / cols) + 0.5);
            // `Polygon::new` closes the ring for us, so the points need not repeat the first.
            let exterior: LineString = (0..RING_VERTICES)
                .map(|k| {
                    let theta = 2.0 * PI * f64::from(k) / f64::from(RING_VERTICES);
                    Coord {
                        x: cx + radius * theta.cos(),
                        y: cy + radius * theta.sin(),
                    }
                })
                .collect();
            let geometry = geo_types::Geometry::from(Polygon::new(exterior, vec![]));
            let mut feature = Feature::from(Geometry::new(GeometryValue::from(&geometry)));
            feature.set_property("id", i);
            feature
        })
        .collect();
    let fc = GeoJson::FeatureCollection(features);

    let mut tmp = tempfile::Builder::new()
        .suffix(".geojson")
        .tempfile()
        .expect("failed to create temp file");
    serde_json::to_writer(&mut tmp, &fc).expect("failed to write GeoJSON");
    tmp.flush().expect("failed to flush temp file");
    tmp
}

fn bench_geojson(c: &mut Criterion) {
    let rt = Runtime::new().expect("failed to build tokio runtime");
    // The handle must outlive both benches: dropping it deletes the backing file.
    let file = synthetic_geojson();
    let path = file.path().to_path_buf();

    // Load path: read + parse + build the R-tree.
    c.bench_function("build_source", |b| {
        b.to_async(&rt).iter(|| async {
            let source =
                GeoJsonSource::new("bench".to_string(), path.clone(), CacheZoomRange::default())
                    .await
                    .expect("failed to build GeoJSON source");
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
        .expect("failed to build GeoJSON source");
    c.bench_function("fetch_tile", |b| {
        b.to_async(&rt).iter(|| async {
            let tile = source
                .get_tile(TileCoord::new_unchecked(1, 1, 0), None)
                .await
                .expect("failed to fetch tile");
            black_box(tile);
        });
    });
}

criterion_group!(benches, bench_geojson);
criterion_main!(benches);
