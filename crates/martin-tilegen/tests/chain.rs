use std::sync::{Arc, Mutex};

use geo_types::Coord;
use martin_tile_utils::Encoding;
use martin_tilegen::plan::{ColumnDef, LayerDef, TableDef};
use martin_tilegen::props::{KeyId, KeyInterner, Prop};
use martin_tilegen::source::{
    Crs, FeatureBatch, FeatureReader, FeatureSource, Geometry, MemorySource, SourceChain, SourceError, SourceFeature,
    SourceResult,
};
use martin_tilegen::{
    EncodedTile, GenerateConfig, LayerGrid, Progress, SortConfig, TileFormat, TileGenResult, TileOrder, TileSink,
    generate,
};

#[test]
fn a_chain_numbers_tables_and_partitions_one_source_after_another() {
    let first = MemorySource {
        batches: vec![
            FeatureBatch {
                table: 0,
                partition: 0,
                first_row: 0,
                crs: Crs::Wgs84,
                features: vec![SourceFeature {
                    id: Some(1),
                    geometry: Geometry::Point(Coord { x: 1.0, y: 1.0 }),
                    props: Vec::new(),
                    relations: Vec::new(),
                }],
            },
            FeatureBatch {
                table: 1,
                partition: 1,
                first_row: 0,
                crs: Crs::Wgs84,
                features: vec![SourceFeature {
                    id: Some(2),
                    geometry: Geometry::Point(Coord { x: 2.0, y: 2.0 }),
                    props: Vec::new(),
                    relations: Vec::new(),
                }],
            },
        ],
    };
    let second = MemorySource {
        batches: vec![FeatureBatch {
            table: 0,
            partition: 0,
            first_row: 0,
            crs: Crs::WebMercator,
            features: vec![SourceFeature {
                id: Some(3),
                geometry: Geometry::Point(Coord { x: 3.0, y: 3.0 }),
                props: Vec::new(),
                relations: Vec::new(),
            }],
        }],
    };
    let grid = LayerGrid {
        extent: 4096,
        buffer: 64,
    };
    let chain = SourceChain::new(vec![
        (
            Box::new(first) as Box<dyn FeatureSource + Send>,
            vec![
                TableDef {
                    columns: Vec::new(),
                    layers: vec![LayerDef::new("places", 0..=4, grid)],
                },
                TableDef {
                    columns: Vec::new(),
                    layers: vec![LayerDef::new("roads", 0..=4, grid)],
                },
            ],
        ),
        (
            Box::new(second),
            vec![TableDef {
                columns: Vec::new(),
                layers: vec![LayerDef::new("ocean", 0..=4, grid)],
            }],
        ),
    ])
    .unwrap();
    let mut read = Vec::new();
    for partition in 0..chain.partitions() {
        chain
            .read(partition, chain.plan().table_keys(), &mut |batch| {
                let ids: Vec<_> = batch.features.iter().map(|f| f.id).collect();
                read.push(format!(
                    "partition {} table {} {:?} {ids:?}",
                    batch.partition, batch.table, batch.crs
                ));
                Ok(())
            })
            .unwrap();
    }
    insta::assert_snapshot!(read.join("\n"), @"
    partition 0 table 0 Wgs84 [Some(1)]
    partition 1 table 1 Wgs84 [Some(2)]
    partition 2 table 2 WebMercator [Some(3)]
    ");
}

#[test]
fn a_chain_refuses_a_partition_past_its_last() {
    let one_partition = || MemorySource {
        batches: vec![FeatureBatch {
            table: 0,
            partition: 0,
            first_row: 0,
            crs: Crs::Wgs84,
            features: Vec::new(),
        }],
    };
    let grid = LayerGrid {
        extent: 4096,
        buffer: 64,
    };
    let chain = SourceChain::new(vec![
        (
            Box::new(one_partition()) as Box<dyn FeatureSource + Send>,
            vec![TableDef {
                columns: Vec::new(),
                layers: vec![LayerDef::new("places", 0..=4, grid)],
            }],
        ),
        (
            Box::new(one_partition()),
            vec![TableDef {
                columns: Vec::new(),
                layers: vec![LayerDef::new("roads", 0..=4, grid)],
            }],
        ),
    ])
    .unwrap();
    let err = chain.read(2, chain.plan().table_keys(), &mut |_| Ok(())).unwrap_err();
    insta::assert_snapshot!(err, @"partition 2 does not exist");
}

#[test]
fn a_chain_reader_reads_each_source_through_that_sources_own_readers() {
    struct Pooled;

    struct PooledReader(i64);

    impl FeatureReader for PooledReader {
        fn read(
            &mut self,
            partition: u32,
            _keys: &[KeyInterner],
            emit: &mut dyn FnMut(FeatureBatch) -> SourceResult<()>,
        ) -> SourceResult<()> {
            emit(FeatureBatch {
                table: 0,
                partition,
                first_row: 0,
                crs: Crs::Wgs84,
                features: vec![SourceFeature {
                    id: Some(self.0),
                    geometry: Geometry::Point(Coord { x: 0.0, y: 0.0 }),
                    props: Vec::new(),
                    relations: Vec::new(),
                }],
            })
        }
    }

    impl FeatureSource for Pooled {
        fn partitions(&self) -> u32 {
            2
        }

        fn read(
            &self,
            partition: u32,
            keys: &[KeyInterner],
            emit: &mut dyn FnMut(FeatureBatch) -> SourceResult<()>,
        ) -> SourceResult<()> {
            PooledReader(0).read(partition, keys, emit)
        }

        fn open_readers(&self, _count: usize) -> SourceResult<Vec<Box<dyn FeatureReader + '_>>> {
            Ok(vec![Box::new(PooledReader(100))])
        }
    }

    let grid = LayerGrid {
        extent: 4096,
        buffer: 64,
    };
    let chain = SourceChain::new(vec![
        (
            Box::new(Pooled) as Box<dyn FeatureSource + Send>,
            vec![TableDef {
                columns: Vec::new(),
                layers: vec![LayerDef::new("pooled", 0..=4, grid)],
            }],
        ),
        (
            Box::new(MemorySource {
                batches: vec![FeatureBatch {
                    table: 0,
                    partition: 0,
                    first_row: 0,
                    crs: Crs::Wgs84,
                    features: Vec::new(),
                }],
            }),
            vec![TableDef {
                columns: Vec::new(),
                layers: vec![LayerDef::new("plain", 0..=4, grid)],
            }],
        ),
    ])
    .unwrap();
    let mut readers = chain.open_readers(2).unwrap();
    let mut read = Vec::new();
    for (reader, partition) in readers.iter_mut().zip([1, 0]) {
        reader
            .read(partition, chain.plan().table_keys(), &mut |batch| {
                read.push(format!("partition {} id {:?}", batch.partition, batch.features[0].id));
                Ok(())
            })
            .unwrap();
    }
    insta::assert_snapshot!(read.join("\n"), @"
    partition 1 id Some(100)
    partition 0 id Some(100)
    ");
}

#[test]
fn a_chain_opens_as_many_readers_as_the_source_that_opens_the_most() {
    struct HalfAsMany(MemorySource);

    impl FeatureSource for HalfAsMany {
        fn partitions(&self) -> u32 {
            self.0.partitions()
        }

        fn read(
            &self,
            partition: u32,
            keys: &[KeyInterner],
            emit: &mut dyn FnMut(FeatureBatch) -> SourceResult<()>,
        ) -> SourceResult<()> {
            self.0.read(partition, keys, emit)
        }

        fn open_readers(&self, count: usize) -> SourceResult<Vec<Box<dyn FeatureReader + '_>>> {
            self.0.open_readers(count.div_ceil(2))
        }
    }

    let chain = SourceChain::new(vec![(
        Box::new(HalfAsMany(MemorySource {
            batches: vec![FeatureBatch {
                table: 0,
                partition: 0,
                first_row: 0,
                crs: Crs::Wgs84,
                features: Vec::new(),
            }],
        })) as Box<dyn FeatureSource + Send>,
        vec![TableDef {
            columns: Vec::new(),
            layers: vec![LayerDef::new(
                "pooled",
                0..=4,
                LayerGrid {
                    extent: 4096,
                    buffer: 64,
                },
            )],
        }],
    )])
    .unwrap();
    assert_eq!(chain.open_readers(8).unwrap().len(), 4);
}

#[test]
fn a_chain_gives_each_source_the_keys_of_its_own_tables() {
    struct Tagging;

    impl FeatureSource for Tagging {
        fn partitions(&self) -> u32 {
            1
        }

        fn read(
            &self,
            partition: u32,
            keys: &[KeyInterner],
            emit: &mut dyn FnMut(FeatureBatch) -> SourceResult<()>,
        ) -> SourceResult<()> {
            emit(FeatureBatch {
                table: 0,
                partition,
                first_row: 0,
                crs: Crs::Wgs84,
                features: vec![SourceFeature {
                    id: Some(1),
                    geometry: Geometry::Point(Coord { x: 0.0, y: 0.0 }),
                    props: vec![(keys[0].intern("surface"), Prop::Bool(true))],
                    relations: Vec::new(),
                }],
            })
        }
    }

    let grid = LayerGrid {
        extent: 4096,
        buffer: 64,
    };
    let chain = SourceChain::new(vec![
        (
            Box::new(MemorySource { batches: Vec::new() }) as Box<dyn FeatureSource + Send>,
            vec![TableDef {
                columns: vec![
                    ColumnDef::Value("name".to_owned(), None),
                    ColumnDef::Value("rank".to_owned(), None),
                ],
                layers: vec![LayerDef::new("places", 0..=4, grid)],
            }],
        ),
        (
            Box::new(Tagging),
            vec![TableDef {
                columns: vec![ColumnDef::Keys("tags".to_owned())],
                layers: vec![LayerDef::new("roads", 0..=4, grid)],
            }],
        ),
    ])
    .unwrap();
    let mut keys = Vec::new();
    chain
        .read(0, chain.plan().table_keys(), &mut |batch| {
            keys.extend(batch.features[0].props.iter().map(|(key, _)| *key));
            Ok(())
        })
        .unwrap();
    assert_eq!(keys, [KeyId::new(1)]);
    assert_eq!(chain.plan().table_keys()[1].intern("surface"), KeyId::new(1));
}

#[test]
fn a_chain_finishes_every_source() {
    struct Unfinished;

    impl FeatureSource for Unfinished {
        fn partitions(&self) -> u32 {
            0
        }

        fn read(
            &self,
            partition: u32,
            _keys: &[KeyInterner],
            _emit: &mut dyn FnMut(FeatureBatch) -> SourceResult<()>,
        ) -> SourceResult<()> {
            Err(SourceError::UnknownPartition(partition))
        }

        fn finish(&self) -> SourceResult<()> {
            Err(SourceError::Other("the file changed while it was read".into()))
        }
    }

    let grid = LayerGrid {
        extent: 4096,
        buffer: 64,
    };
    let chain = SourceChain::new(vec![
        (
            Box::new(MemorySource { batches: Vec::new() }) as Box<dyn FeatureSource + Send>,
            vec![TableDef {
                columns: Vec::new(),
                layers: vec![LayerDef::new("places", 0..=4, grid)],
            }],
        ),
        (
            Box::new(Unfinished),
            vec![TableDef {
                columns: Vec::new(),
                layers: vec![LayerDef::new("roads", 0..=4, grid)],
            }],
        ),
    ])
    .unwrap();
    insta::assert_snapshot!(chain.finish().unwrap_err(), @"the file changed while it was read");
}

#[test]
fn a_chain_estimates_its_features_only_if_every_source_does() {
    struct Unknown;

    impl FeatureSource for Unknown {
        fn partitions(&self) -> u32 {
            0
        }

        fn read(
            &self,
            partition: u32,
            _keys: &[KeyInterner],
            _emit: &mut dyn FnMut(FeatureBatch) -> SourceResult<()>,
        ) -> SourceResult<()> {
            Err(SourceError::UnknownPartition(partition))
        }
    }

    let grid = LayerGrid {
        extent: 4096,
        buffer: 64,
    };
    let point = |id| SourceFeature {
        id: Some(id),
        geometry: Geometry::Point(Coord { x: 0.0, y: 0.0 }),
        props: Vec::new(),
        relations: Vec::new(),
    };
    let known = || MemorySource {
        batches: vec![FeatureBatch {
            table: 0,
            partition: 0,
            first_row: 0,
            crs: Crs::Wgs84,
            features: vec![point(1), point(2)],
        }],
    };
    let both_known = SourceChain::new(vec![
        (
            Box::new(known()) as Box<dyn FeatureSource + Send>,
            vec![TableDef {
                columns: Vec::new(),
                layers: vec![LayerDef::new("places", 0..=4, grid)],
            }],
        ),
        (
            Box::new(known()),
            vec![TableDef {
                columns: Vec::new(),
                layers: vec![LayerDef::new("roads", 0..=4, grid)],
            }],
        ),
    ])
    .unwrap();
    let one_unknown = SourceChain::new(vec![
        (
            Box::new(known()) as Box<dyn FeatureSource + Send>,
            vec![TableDef {
                columns: Vec::new(),
                layers: vec![LayerDef::new("places", 0..=4, grid)],
            }],
        ),
        (
            Box::new(Unknown),
            vec![TableDef {
                columns: Vec::new(),
                layers: vec![LayerDef::new("roads", 0..=4, grid)],
            }],
        ),
    ])
    .unwrap();
    assert_eq!(both_known.estimated_features(), Some(4));
    assert_eq!(one_unknown.estimated_features(), None);
}

#[test]
fn chained_sources_render_into_one_tileset() {
    struct Layers(Arc<Mutex<Vec<String>>>);

    impl TileSink for Layers {
        fn tile_order(&self) -> TileOrder {
            TileOrder::Tms
        }

        fn write_all(
            &mut self,
            batches: &mut dyn Iterator<Item = TileGenResult<Vec<EncodedTile>>>,
        ) -> TileGenResult<()> {
            for batch in batches {
                batch?;
            }
            Ok(())
        }

        fn finish(self, metadata: &tilejson::TileJSON) -> TileGenResult<()> {
            let layers = metadata.vector_layers.iter().flatten();
            *self.0.lock().unwrap() = layers.map(|layer| layer.id.clone()).collect();
            Ok(())
        }
    }

    let grid = LayerGrid {
        extent: 4096,
        buffer: 64,
    };
    let point = |id, x| SourceFeature {
        id: Some(id),
        geometry: Geometry::Point(Coord { x, y: 10.0 }),
        props: Vec::new(),
        relations: Vec::new(),
    };
    let chain = SourceChain::new(vec![
        (
            Box::new(MemorySource {
                batches: vec![FeatureBatch {
                    table: 0,
                    partition: 0,
                    first_row: 0,
                    crs: Crs::Wgs84,
                    features: vec![point(1, 10.0)],
                }],
            }) as Box<dyn FeatureSource + Send>,
            vec![TableDef {
                columns: Vec::new(),
                layers: vec![LayerDef::new("places", 0..=2, grid)],
            }],
        ),
        (
            Box::new(MemorySource {
                batches: vec![FeatureBatch {
                    table: 0,
                    partition: 0,
                    first_row: 0,
                    crs: Crs::WebMercator,
                    features: vec![point(2, 2_000_000.0), point(3, 3_000_000.0)],
                }],
            }),
            vec![TableDef {
                columns: Vec::new(),
                layers: vec![LayerDef::new("ocean", 0..=2, grid)],
            }],
        ),
    ])
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let layers = Arc::new(Mutex::new(Vec::new()));
    let summary = generate(
        &chain,
        chain.plan(),
        Layers(Arc::clone(&layers)),
        &GenerateConfig {
            threads: 2,
            sort: SortConfig {
                temp_dirs: vec![dir.path().to_path_buf()],
                buffer_bytes: 16 << 10,
                max_fan_in: 4,
            },
            format: TileFormat::Mvt,
            encoding: Encoding::Uncompressed,
            batch_tiles: 4,
            window: 2,
            mvt_preserve_ring_start: false,
            mvt_keep_unused_keys: false,
        },
        tilejson::tilejson! { tiles: vec![] },
        &Progress::default(),
    )
    .unwrap();
    assert_eq!(summary.features, 3);
    insta::assert_debug_snapshot!(layers.lock().unwrap(), @r#"
    [
        "places",
        "ocean",
    ]
    "#);
}

#[test]
fn chained_sources_write_layers_of_one_name_into_one_layer() {
    struct Metadata(Arc<Mutex<Vec<String>>>);

    impl TileSink for Metadata {
        fn tile_order(&self) -> TileOrder {
            TileOrder::Tms
        }

        fn write_all(
            &mut self,
            batches: &mut dyn Iterator<Item = TileGenResult<Vec<EncodedTile>>>,
        ) -> TileGenResult<()> {
            for batch in batches {
                batch?;
            }
            Ok(())
        }

        fn finish(self, metadata: &tilejson::TileJSON) -> TileGenResult<()> {
            let layers = metadata.vector_layers.iter().flatten();
            *self.0.lock().unwrap() = layers
                .map(|layer| {
                    format!(
                        "{} {:?} z{:?}-{:?}",
                        layer.id, layer.fields, layer.minzoom, layer.maxzoom
                    )
                })
                .collect();
            Ok(())
        }
    }

    let grid = LayerGrid {
        extent: 4096,
        buffer: 64,
    };
    let point = |id, x, value: &str| SourceFeature {
        id: Some(id),
        geometry: Geometry::Point(Coord { x, y: 10.0 }),
        props: vec![(KeyId::new(0), Prop::Str(value.into()))],
        relations: Vec::new(),
    };
    let chain = SourceChain::new(vec![
        (
            Box::new(MemorySource {
                batches: vec![FeatureBatch {
                    table: 0,
                    partition: 0,
                    first_row: 0,
                    crs: Crs::Wgs84,
                    features: vec![point(1, 10.0, "lake")],
                }],
            }) as Box<dyn FeatureSource + Send>,
            vec![TableDef {
                columns: vec![ColumnDef::Value("kind".to_owned(), None)],
                layers: vec![LayerDef::new("water", 1..=3, grid)],
            }],
        ),
        (
            Box::new(MemorySource {
                batches: vec![FeatureBatch {
                    table: 0,
                    partition: 0,
                    first_row: 0,
                    crs: Crs::Wgs84,
                    features: vec![point(2, 20.0, "Atlantic")],
                }],
            }),
            vec![TableDef {
                columns: vec![ColumnDef::Value("name".to_owned(), None)],
                layers: vec![LayerDef::new("water", 0..=2, grid)],
            }],
        ),
    ])
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let layers = Arc::new(Mutex::new(Vec::new()));
    generate(
        &chain,
        chain.plan(),
        Metadata(Arc::clone(&layers)),
        &GenerateConfig {
            threads: 2,
            sort: SortConfig {
                temp_dirs: vec![dir.path().to_path_buf()],
                buffer_bytes: 16 << 10,
                max_fan_in: 4,
            },
            format: TileFormat::Mvt,
            encoding: Encoding::Uncompressed,
            batch_tiles: 4,
            window: 2,
            mvt_preserve_ring_start: false,
            mvt_keep_unused_keys: false,
        },
        tilejson::tilejson! { tiles: vec![] },
        &Progress::default(),
    )
    .unwrap();
    insta::assert_debug_snapshot!(layers.lock().unwrap(), @r#"
    [
        "water {\"kind\": \"String\", \"name\": \"String\"} zSome(0)-Some(3)",
    ]
    "#);
}
