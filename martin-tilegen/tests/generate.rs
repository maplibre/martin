//! End to end: a synthetic multi-layer, multi-partition source generated into `MBTiles`.
#![cfg(feature = "mbtiles")]
#![expect(clippy::unwrap_used)]

use std::path::Path;

use geo_types::{Coord, LineString, Polygon};
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

/// Six partitions alternating between a points layer and a lines layer, in WGS84, and one partition
/// of polygons: a large one with a hole, whose covered tiles become fill ranges, and a small one.
fn source() -> MemorySource {
    let mut batches: Vec<_> = (0..6u32)
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
    let rect = |w: f64, s: f64, e: f64, n: f64| {
        LineString::from(vec![(w, s), (e, s), (e, n), (w, n), (w, s)])
    };
    let parks = [
        Polygon::new(
            rect(-100.0, -40.0, 60.0, 50.0),
            vec![rect(-30.0, -10.0, 10.0, 20.0)],
        ),
        Polygon::new(rect(100.0, 10.0, 104.0, 13.0), vec![]),
    ];
    batches.push(FeatureBatch {
        layer: 2,
        partition: 6,
        first_row: 0,
        crs: Crs::Wgs84,
        features: parks
            .into_iter()
            .zip(1..)
            .map(|(polygon, id)| SourceFeature {
                id: Some(id),
                geometry: Geometry::Polygons(vec![polygon]),
                props: vec![(KeyId::from(0), Prop::Str(format!("park {id}")))],
            })
            .collect(),
    });
    MemorySource {
        layers: vec![
            layer("places", &["name", "rank"]),
            layer("roads", &["name", "rank"]),
            layer("parks", &["name"]),
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
    assert_eq!(summary.features, 122);
    assert_eq!(summary.slice_errors, 0);
    assert_eq!(summary.tiles, rows.len() as u64);
    assert!(rows.iter().any(|r| r.0 == 0) && rows.iter().any(|r| r.0 == 4));

    // The single z0 tile holds every feature of both layers, in source order.
    let z0 = decode(&rows[0].3);
    assert_eq!(
        z0.iter().map(TileLayer::name).collect::<Vec<_>>(),
        ["places", "roads", "parks"]
    );
    assert_eq!(z0[2].features().len(), 2);
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
        ["places", "roads", "parks"]
    );
    assert_eq!(layers[0].fields["rank"], "Number");

    // Interior tiles of the big park are fill squares, stored once however many tiles they are.
    let fills = rows
        .iter()
        .filter(|r| r.0 == 4)
        .filter(|r| {
            decode(&r.3).iter().any(|layer| {
                layer.name() == "parks"
                    && matches!(layer.features()[0].geometry(), geo_types::Geometry::Polygon(p)
                        if p.exterior().0[0] == Coord { x: -64, y: -64 })
            })
        })
        .count();
    assert!(fills > 10, "{fills} fill tiles at z4");
    let fill_bytes: std::collections::HashSet<_> =
        rows.iter().filter(|r| r.0 == 4).map(|r| &r.3).collect();
    assert!(
        fill_bytes.len() < rows.iter().filter(|r| r.0 == 4).count(),
        "identical tiles repeat"
    );

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
