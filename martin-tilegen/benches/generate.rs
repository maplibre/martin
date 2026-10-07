#![cfg_attr(not(target_os = "linux"), allow(unused))]

#[cfg(target_os = "linux")]
#[path = "support/cycles.rs"]
mod cycles;

use std::f64::consts::TAU;
use std::hint::black_box;

use criterion::{Criterion, Throughput};
use geo_types::{Coord, LineString, Polygon};
use martin_tile_utils::Encoding;
use martin_tilegen::props::KeyId;
use martin_tilegen::source::{
    Crs, FeatureBatch, FeatureSource, Geometry, LayerSpec, MemorySource, Prop, SourceFeature,
};
use martin_tilegen::{
    EncodedTile, FeatureOrder, GenerateConfig, LayerGrid, Progress, SortConfig, TileFormat,
    TileGenResult, TileOrder, TileSink, generate,
};
use mlt_core::encoder::EncoderConfig;
use tilejson::TileJSON;

const PARTITIONS: u32 = 8;
const CENTER: Coord<f64> = Coord { x: 13.4, y: 52.5 };

struct Rng(u64);

impl Rng {
    /// Uniform in `-1..1`.
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        #[expect(clippy::cast_precision_loss, reason = "53 random bits")]
        let unit = (self.0 >> 11) as f64 / (1u64 << 53) as f64;
        unit * 2.0 - 1.0
    }

    fn near(&mut self, center: Coord<f64>, radius: f64) -> Coord<f64> {
        Coord {
            x: center.x + self.next() * radius,
            y: center.y + self.next() * radius * 0.6,
        }
    }
}

fn ring(center: Coord<f64>, radius: f64, vertices: u32, rng: &mut Rng) -> LineString<f64> {
    (0..=vertices)
        .map(|i| {
            let angle = TAU * f64::from(i % vertices) / f64::from(vertices);
            let r = radius * (1.0 + 0.2 * rng.next());
            (center.x + r * angle.cos(), center.y + r * angle.sin() * 0.6)
        })
        .collect()
}

/// A city: buildings, roads, points of interest and land use around `CENTER`, plus one lake whose
/// interior covers thousands of z14 tiles.
fn city() -> Vec<(LayerSpec, Vec<SourceFeature>)> {
    let mut rng = Rng(0x1234_5678);
    let mut layers: Vec<(LayerSpec, Vec<SourceFeature>)> = Vec::new();
    let mut layer = |name: &str, keys: &[&str], features: Vec<SourceFeature>| {
        let spec = LayerSpec {
            name: name.to_owned(),
            zooms: 0..=14,
            grid: LayerGrid {
                extent: 4096,
                buffer: 64,
            },
            clip: true,
            order: FeatureOrder::Source,
            known_keys: keys.iter().map(|&k| k.to_owned()).collect(),
            bounds: None,
        };
        layers.push((spec, features));
    };
    let feature = |id: u32, geometry, props| SourceFeature {
        id: Some(u64::from(id)),
        geometry,
        props,
    };
    let (k0, k1) = (KeyId::from(0), KeyId::from(1));
    let names = ["Main St", "Oak Ave", "Elm Rd", "Hauptstraße"];

    let buildings = (0..60_000)
        .map(|i| {
            let outline = ring(rng.near(CENTER, 0.15), 0.0002, 5, &mut rng);
            let props = vec![
                (k0, Prop::I64(i64::from(i % 30))),
                (k1, Prop::Str("house".into())),
            ];
            feature(
                i,
                Geometry::Polygons(vec![Polygon::new(outline, vec![])]),
                props,
            )
        })
        .collect();
    layer("buildings", &["height", "kind"], buildings);

    let roads = (0..15_000)
        .map(|i| {
            let mut at = rng.near(CENTER, 0.15);
            let line = (0..20)
                .map(|_| {
                    at = rng.near(at, 0.001);
                    at
                })
                .collect();
            let props = vec![
                (k0, Prop::Str(names[i as usize % 4].into())),
                (k1, Prop::Str("residential".into())),
            ];
            feature(i, Geometry::Lines(vec![line]), props)
        })
        .collect();
    layer("roads", &["name", "class"], roads);

    let pois = (0..20_000)
        .map(|i| {
            let props = vec![
                (k0, Prop::Str(format!("poi {i}"))),
                (k1, Prop::I64(i64::from(i % 5))),
            ];
            feature(i, Geometry::Points(vec![rng.near(CENTER, 0.15)]), props)
        })
        .collect();
    layer("pois", &["name", "rank"], pois);

    let mut landuse: Vec<_> = (0..300)
        .map(|i| {
            let outline = ring(rng.near(CENTER, 0.15), 0.01, 40, &mut rng);
            feature(
                i,
                Geometry::Polygons(vec![Polygon::new(outline, vec![])]),
                vec![(k0, Prop::Str("park".into()))],
            )
        })
        .collect();
    let lake = ring(Coord { x: 14.2, y: 52.5 }, 0.6, 2000, &mut rng);
    landuse.push(feature(
        300,
        Geometry::Polygons(vec![Polygon::new(lake, vec![])]),
        vec![(k0, Prop::Str("water".into()))],
    ));
    layer("landuse", &["kind"], landuse);
    layers
}

/// Splits each layer into `PARTITIONS` batches, read in parallel.
fn partitioned(layers: Vec<(LayerSpec, Vec<SourceFeature>)>) -> (MemorySource, u64) {
    let features = layers.iter().map(|(_, f)| f.len() as u64).sum();
    let mut specs = Vec::new();
    let mut batches = Vec::new();
    for (index, (spec, features)) in layers.into_iter().enumerate() {
        let per_partition = features.len().div_ceil(PARTITIONS as usize);
        let mut features = features.into_iter();
        loop {
            let chunk: Vec<_> = features.by_ref().take(per_partition).collect();
            if chunk.is_empty() {
                break;
            }
            batches.push(FeatureBatch {
                layer: u8::try_from(index).expect("four layers"),
                partition: 0,
                first_row: 0,
                crs: Crs::Wgs84,
                features: chunk,
            });
        }
        specs.push(spec);
    }
    let source = MemorySource {
        layers: specs,
        batches,
    };
    (source, features)
}

/// Drops the tiles, so the bench measures generation, not storage.
struct NullSink;

impl TileSink for NullSink {
    fn tile_order(&self) -> TileOrder {
        TileOrder::Hilbert
    }

    fn write_all(
        &mut self,
        batches: &mut dyn Iterator<Item = TileGenResult<Vec<EncodedTile>>>,
    ) -> TileGenResult<()> {
        for batch in batches {
            black_box(batch?);
        }
        Ok(())
    }

    fn finish(self, metadata: &TileJSON) -> TileGenResult<()> {
        black_box(metadata);
        Ok(())
    }
}

fn run(source: &impl FeatureSource, format: TileFormat, temp: &std::path::Path) -> u64 {
    let config = GenerateConfig {
        threads: 4,
        sort: SortConfig {
            temp_dirs: vec![temp.to_path_buf()],
            buffer_bytes: 64 << 20,
            max_fan_in: 256,
            read_buffer_bytes: 256 << 10,
        },
        format,
        encoding: Encoding::Uncompressed,
        batch_tiles: 256,
        window: 16,
    };
    let metadata = tilejson::tilejson! { tiles: vec![] };
    generate(source, NullSink, &config, metadata, &Progress::default())
        .expect("generate")
        .tiles
}

fn bench(c: &mut Criterion<cycles::Cycles>) {
    let (source, features) = partitioned(city());
    let temp = tempfile::tempdir().expect("temp dir");
    let mut group = c.benchmark_group("generate");
    group
        .sample_size(10)
        .throughput(Throughput::Elements(features));
    for (name, format) in [
        ("mlt", TileFormat::Mlt(EncoderConfig::default())),
        ("mvt", TileFormat::Mvt),
    ] {
        group.bench_function(name, |b| b.iter(|| run(&source, format, temp.path())));
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
