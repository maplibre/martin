#![expect(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    reason = "small synthetic values"
)]
#![cfg_attr(
    not(target_os = "linux"),
    allow(unused, reason = "the cycle counter is linux-only")
)]

#[cfg(target_os = "linux")]
#[path = "support/cycles.rs"]
mod cycles;

use std::hint::black_box;

use criterion::{BatchSize, Criterion, Throughput};
use martin_tile_utils::Encoding;
use martin_tilegen::props::{KeyInterner, KeyNames, PropRef};
use martin_tilegen::record::{EncodedProps, Geom, encode};
use martin_tilegen::{
    DedupIndex, EncodeSettings, FeatureOrder, LayerGrid, LayerInfo, Seq, TileEncoder, TileFormat,
    TileOrder, TileRecords,
};
use mlt_core::encoder::EncoderConfig;

const TILES: u64 = 100;
const FEATURES: u64 = 200;

/// Tiles of 200 small building-like polygons with a few properties, all distinct, so no tile is reused.
fn tiles() -> (Vec<TileRecords>, KeyNames) {
    let interner = KeyInterner::new(["name", "height", "kind"]);
    let mut props = EncodedProps::default();
    let mut bytes = Vec::new();
    let tiles = (0..TILES)
        .map(|tile_id| {
            let mut tile = TileRecords::new(tile_id);
            for f in 0..FEATURES {
                let (x, y) = (
                    (f % 20 * 200) as i32,
                    (f / 20 * 400) as i32 + tile_id as i32,
                );
                let vertices = [
                    [x, y],
                    [x + 150, y],
                    [x + 150, y + 90],
                    [x + 70, y + 130],
                    [x, y + 90],
                ];
                props.clear();
                props.push(
                    interner.intern("name"),
                    PropRef::Str(["Main St", "Oak Ave", "Elm Rd"][f as usize % 3]),
                );
                props.push(interner.intern("height"), PropRef::I64((f % 40) as i64));
                props.push(interner.intern("kind"), PropRef::Str("building"));
                bytes.clear();
                encode(
                    &mut bytes,
                    Some(f),
                    &props,
                    Geom::Polygons {
                        polygons: &[1],
                        rings: &[5],
                        vertices: &vertices,
                    },
                );
                tile.push_record(0, Seq::new(0, f).expect("row fits"), &bytes);
            }
            tile
        })
        .collect();
    (tiles, interner.freeze())
}

#[cfg(target_os = "linux")]
fn encode_tiles(c: &mut Criterion<cycles::Cycles>) {
    let (tiles, names) = tiles();
    let keys = [names];
    let mlt = TileFormat::Mlt(EncoderConfig::default());
    let mut group = c.benchmark_group("encode");
    group
        .throughput(Throughput::Elements(TILES))
        .sample_size(10);
    for (name, format, order, encoding) in [
        (
            "mlt_source",
            mlt,
            FeatureOrder::Source,
            Encoding::Uncompressed,
        ),
        ("mlt_auto", mlt, FeatureOrder::Auto, Encoding::Uncompressed),
        (
            "mvt_source",
            TileFormat::Mvt,
            FeatureOrder::Source,
            Encoding::Uncompressed,
        ),
        ("mlt_source_gzip", mlt, FeatureOrder::Source, Encoding::Gzip),
    ] {
        let layers = [LayerInfo {
            name: "buildings".to_owned(),
            grid: LayerGrid {
                extent: 4096,
                buffer: 64,
            },
            order,
        }];
        let settings = EncodeSettings {
            layers: &layers,
            keys: &keys,
            format,
            encoding,
            order: TileOrder::Tms,
        };
        group.bench_function(name, |b| {
            b.iter_batched(
                || tiles.clone(),
                |batch| {
                    let encoded = TileEncoder::default()
                        .encode_batch(&settings, &DedupIndex::default(), batch)
                        .expect("encode");
                    black_box(encoded);
                },
                BatchSize::LargeInput,
            );
        });
    }
    group.finish();
}

#[cfg(target_os = "linux")]
criterion::criterion_group! {
    name = benches;
    config = Criterion::default().with_measurement(cycles::Cycles);
    targets = encode_tiles
}
#[cfg(target_os = "linux")]
criterion::criterion_main!(benches);

/// Cycle counting uses Linux `perf_event_open`.
#[cfg(not(target_os = "linux"))]
fn main() {}
