#![cfg_attr(not(target_os = "linux"), allow(unused))]

#[cfg(target_os = "linux")]
#[path = "support/cycles.rs"]
mod cycles;

use std::f64::consts::TAU;

use criterion::{BatchSize, Criterion, Throughput};
use geo_types::{Coord, LineString, Polygon};
use martin_tilegen::internals::record::EncodedProps;
use martin_tilegen::internals::{Renderer, Seq, Sorter};
use martin_tilegen::project::from_lonlat;
use martin_tilegen::{
    Feature, FeatureGeom, LayerGrid, PixelThreshold, RenderLayer, SortConfig, TileOrder,
};

const VERTICES: u32 = 10_000;

/// A xorshift generator, so every run renders the same geometry.
struct Rng(u64);

impl Rng {
    /// Returns a uniform value in `-1..1`.
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        #[expect(clippy::cast_precision_loss, reason = "53 random bits")]
        let unit = (self.0 >> 11) as f64 / (1u64 << 53) as f64;
        unit * 2.0 - 1.0
    }
}

/// A 10k-vertex walk of 10 m steps whose heading drifts, like a coastline.
fn coastline(start: Coord<f64>, rng: &mut Rng) -> LineString<f64> {
    let (mut at, mut heading) = (start, rng.next() * TAU);
    (0..VERTICES)
        .map(|_| {
            heading += rng.next() * 0.3;
            at.x += heading.cos() * 1e-4;
            at.y += heading.sin() * 0.6e-4;
            from_lonlat(at)
        })
        .collect()
}

/// A 10k-vertex ring about 0.4° across with a wavy, noisy shore.
fn lake(center: Coord<f64>, rng: &mut Rng) -> Polygon<f64> {
    let shore = (0..=VERTICES)
        .map(|i| {
            let angle = TAU * f64::from(i % VERTICES) / f64::from(VERTICES);
            let r = 0.2 * (1.0 + 0.05 * (7.0 * angle).sin() + 0.01 * rng.next());
            from_lonlat(Coord {
                x: center.x + r * angle.cos(),
                y: center.y + r * angle.sin() * 0.6,
            })
        })
        .collect();
    Polygon::new(shore, vec![])
}

/// Renders the coastlines and the lakes at z0-14 into an in-memory sort buffer.
fn bench(c: &mut Criterion<cycles::Cycles>) {
    let mut rng = Rng(0x1234_5678);
    let lines: Vec<_> = (0..16)
        .map(|i| {
            coastline(
                Coord {
                    x: 8.0 + f64::from(i) * 0.3,
                    y: 54.0,
                },
                &mut rng,
            )
        })
        .collect();
    let polygons: Vec<_> = (0..4)
        .map(|i| {
            lake(
                Coord {
                    x: 10.0 + f64::from(i),
                    y: 52.0,
                },
                &mut rng,
            )
        })
        .collect();
    let layer = RenderLayer::new(
        0,
        0..=14,
        LayerGrid {
            extent: 4096,
            buffer: 64,
        },
    )
    .expect("a valid layer");
    let props = EncodedProps::default();
    let dir = tempfile::tempdir().expect("temp dir");
    let sorter = Sorter::new(SortConfig {
        temp_dirs: vec![dir.path().to_path_buf()],
        buffer_bytes: 1 << 30,
        max_fan_in: 256,
    })
    .expect("valid config");

    let mut group = c.benchmark_group("render");
    group.sample_size(20);
    for (name, geoms) in [
        (
            "coastlines",
            lines
                .iter()
                .map(std::slice::from_ref)
                .map(FeatureGeom::Lines)
                .collect::<Vec<_>>(),
        ),
        (
            "lakes",
            polygons
                .iter()
                .map(std::slice::from_ref)
                .map(FeatureGeom::Polygons)
                .collect(),
        ),
    ] {
        group.throughput(Throughput::Elements(
            u64::from(VERTICES) * geoms.len() as u64,
        ));
        group.bench_function(name, |b| {
            let mut renderer = Renderer::default();
            b.iter_batched(
                || sorter.buffer(),
                |mut buffer| {
                    for (row, &geom) in geoms.iter().enumerate() {
                        let feature = Feature {
                            id: Some(row as u64),
                            geom,
                            props: &props,
                            bands: &[],
                            sort: None,
                            zooms: 0..=14,
                            simplify: PixelThreshold::DEFAULT_SIMPLIFY,
                            min_size: PixelThreshold::DEFAULT_MIN_SIZE,
                        };
                        let seq = Seq::new(0, row as u64).expect("row fits");
                        renderer
                            .render(TileOrder::Hilbert, &layer, seq, &feature, &mut buffer)
                            .expect("render");
                    }
                    buffer
                },
                BatchSize::PerIteration,
            );
        });
    }
    group.finish();
}

#[cfg(target_os = "linux")]
criterion::criterion_group! {
    name = benches;
    config = Criterion::default().with_measurement(cycles::Cycles);
    targets = bench
}
#[cfg(target_os = "linux")]
criterion::criterion_main!(benches);

/// Cycle counting uses Linux `perf_event_open`.
#[cfg(not(target_os = "linux"))]
fn main() {}
