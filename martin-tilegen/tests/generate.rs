//! End to end: a synthetic multi-layer, multi-partition source generated into `MBTiles`.
#![cfg(feature = "mbtiles")]
#![expect(clippy::unwrap_used)]

use std::path::Path;

use geo_types::{Coord, LineString};
use martin_tile_utils::{Encoding, decode_gzip};
use martin_tilegen::props::{KeyId, KeyInterner};
use martin_tilegen::source::{
    Crs, FeatureBatch, FeatureSource, Geometry, LayerSpec, MemorySource, Prop, SourceFeature,
};
use martin_tilegen::{
    FeatureOrder, GenerateConfig, LayerGrid, MbtilesSink, Progress, SortConfig, Summary,
    TileFormat, generate,
};
use mbtiles::Mbtiles;
use mlt_core::encoder::EncoderConfig;
use mlt_core::{Decoder, Parser, TileLayer};

fn layer(name: &str, keys: &[&str]) -> LayerSpec {
    LayerSpec {
        name: name.to_owned(),
        zooms: 0..=4,
        grid: LayerGrid {
            extent: 4096,
            buffer: 64,
        },
        clip: true,
        order: FeatureOrder::Source,
        known_keys: keys.iter().map(|&k| k.to_owned()).collect(),
        bounds: None,
    }
}

/// Six partitions alternating between a points layer and a lines layer, in WGS84.
fn source() -> MemorySource {
    let batches = (0..6u32)
        .map(|p| {
            let base = f64::from(p) * 20.0 - 60.0;
            let features = (0..20u32)
                .map(|i| {
                    let (x, y) = (base + f64::from(i), f64::from(i) * 3.0 - 30.0);
                    let geometry = if p % 2 == 0 {
                        Geometry::Points(vec![Coord { x, y }])
                    } else {
                        Geometry::Lines(vec![LineString::from(vec![
                            (x, y),
                            (x + 5.0, y + 4.0),
                            (x + 9.0, y),
                        ])])
                    };
                    SourceFeature {
                        id: Some(u64::from(p * 100 + i)),
                        geometry,
                        props: vec![
                            (KeyId::from(0), Prop::Str(format!("f{p}-{i}"))),
                            (KeyId::from(1), Prop::I64(i64::from(i))),
                        ],
                    }
                })
                .collect();
            FeatureBatch {
                layer: u8::try_from(p % 2).unwrap(),
                partition: p,
                first_row: 0,
                crs: Crs::Wgs84,
                features,
            }
        })
        .collect();
    MemorySource {
        layers: vec![
            layer("places", &["name", "rank"]),
            layer("roads", &["name", "rank"]),
        ],
        batches,
    }
}

/// `(zoom_level, tile_column, tile_row, tile_data)`
type Row = (i64, i64, i64, Vec<u8>);

fn run(source: &impl FeatureSource, dir: &Path, threads: usize) -> (Summary, Vec<Row>, Mbtiles) {
    let path = dir.join(format!("out-{threads}.mbtiles"));
    let config = GenerateConfig {
        threads,
        sort: SortConfig {
            temp_dirs: vec![dir.to_path_buf()],
            buffer_bytes: 16 << 10,
            max_fan_in: 4,
            read_buffer_bytes: 4096,
        },
        format: TileFormat::Mlt(EncoderConfig::default()),
        encoding: Encoding::Gzip,
        batch_tiles: 7,
        window: 3,
    };
    let summary = generate(
        source,
        MbtilesSink::create(&path).unwrap(),
        &config,
        tilejson::tilejson! { tiles: vec![] },
        &Progress::default(),
    )
    .unwrap();
    let mbt = Mbtiles::new(&path).unwrap();
    let rows = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(async {
            let mut conn = mbt.open_readonly().await.unwrap();
            mbtiles::sqlx::query_as(
                "SELECT zoom_level, tile_column, tile_row, tile_data FROM tiles ORDER BY 1, 2, 3",
            )
            .fetch_all(&mut conn)
            .await
            .unwrap()
        });
    (summary, rows, mbt)
}

fn decode(gzipped: &[u8]) -> Vec<TileLayer> {
    let raw = decode_gzip(gzipped).unwrap();
    let mut decoder = Decoder::default();
    Parser::default()
        .parse_layers(&raw)
        .unwrap()
        .into_iter()
        .map(|l| l.into_tile(&mut decoder).unwrap().unwrap())
        .collect()
}

#[test]
fn generates_a_deterministic_tileset() {
    let source = source();
    let dir = tempfile::tempdir().unwrap();
    let (summary, rows, mbt) = run(&source, dir.path(), 1);
    assert_eq!(summary.features, 120);
    assert_eq!(summary.slice_errors, 0);
    assert_eq!(summary.tiles, rows.len() as u64);
    assert!(rows.iter().any(|r| r.0 == 0) && rows.iter().any(|r| r.0 == 4));

    // The single z0 tile holds every feature of both layers, in source order.
    let z0 = decode(&rows[0].3);
    assert_eq!(
        z0.iter().map(TileLayer::name).collect::<Vec<_>>(),
        ["places", "roads"]
    );
    assert_eq!(z0[0].features().len(), 60);
    let ids: Vec<_> = z0[1].features().iter().map(|f| f.id().unwrap()).collect();
    assert!(ids.is_sorted(), "source order is draw order: {ids:?}");
    assert_eq!(z0[1].property_names(), ["name", "rank"]);

    let metadata = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(async {
            let mut conn = mbt.open_readonly().await.unwrap();
            mbt.get_metadata(&mut conn).await.unwrap()
        });
    assert_eq!(metadata.tilejson.minzoom, Some(0));
    assert_eq!(metadata.tilejson.maxzoom, Some(4));
    let layers = metadata.tilejson.vector_layers.unwrap();
    assert_eq!(
        layers.iter().map(|l| l.id.as_str()).collect::<Vec<_>>(),
        ["places", "roads"]
    );
    assert_eq!(layers[0].fields["rank"], "Number");

    let (parallel, parallel_rows, _) = run(&source, dir.path(), 4);
    assert_eq!(parallel.tiles, summary.tiles);
    assert!(
        parallel_rows == rows,
        "output must not depend on the thread count"
    );
}

#[test]
fn interner_ids_match_known_keys() {
    let keys = KeyInterner::new(["name", "rank"]);
    assert_eq!(keys.intern("name"), KeyId::from(0));
    assert_eq!(keys.intern("rank"), KeyId::from(1));
}
