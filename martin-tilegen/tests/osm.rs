//! An OSM PBF file read with its layers' filters pushed down gives exactly the objects a whole read gives that the
//! layers take.
#![expect(clippy::unwrap_used)]

use std::collections::BTreeSet;
use std::path::PathBuf;

use martin_tilegen::LayerGrid;
use martin_tilegen::osm::{NodeStore, Objects, OsmSource, OsmTable, columns};
use martin_tilegen::plan::{GeometryType, LayerDef, TableDef};
use martin_tilegen::source::{FeatureSource as _, Geometry, SourceFeature};

const GRID: LayerGrid = LayerGrid {
    extent: 4096,
    buffer: 64,
};

const TAGS: [&str; 6] = ["highway", "building", "name", "amenity", "natural", "landuse"];

fn source(filter: Option<&str>, geometry: Option<GeometryType>) -> OsmSource {
    let tags: Vec<String> = TAGS.iter().map(|t| (*t).to_owned()).collect();
    let layer = LayerDef {
        filter: filter.map(str::to_owned),
        geometry,
        ..LayerDef::new("layer", 0..=14, GRID)
    };
    let table = OsmTable {
        path: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/osm/monaco.osm.pbf"),
        node_store: NodeStore::Memory,
        bounds: None,
        objects: Objects::Taken,
    };
    let def = TableDef {
        columns: columns(&tags),
        layers: vec![layer],
    };
    OsmSource::new(vec![(def, table)], 16 << 10).unwrap()
}

fn read_all(source: &OsmSource) -> Vec<SourceFeature> {
    let mut features = Vec::new();
    for partition in 0..source.partitions() {
        source
            .read(partition, source.plan().table_keys(), &mut |batch| {
                features.extend(batch.features);
                Ok(())
            })
            .unwrap();
    }
    features
}

fn taken_by(source: &OsmSource, features: &[SourceFeature]) -> BTreeSet<(i64, bool)> {
    let mut evaluator = source.plan().evaluator();
    let mut taken = BTreeSet::new();
    for feature in features {
        let geom = feature.geometry.feature_geom().unwrap();
        evaluator
            .evaluate(0, feature, geom, |_| {
                taken.insert((feature.id.unwrap(), matches!(feature.geometry, Geometry::Outline(_))));
                Ok(())
            })
            .unwrap();
    }
    taken
}

fn ids(features: &[SourceFeature]) -> BTreeSet<(i64, bool)> {
    features
        .iter()
        .map(|f| (f.id.unwrap(), matches!(f.geometry, Geometry::Outline(_))))
        .collect()
}

#[track_caller]
fn assert_pushdown_exact(filter: &str, geometry: Option<GeometryType>) {
    let tags: Vec<String> = TAGS.iter().map(|t| (*t).to_owned()).collect();
    let table = OsmTable {
        path: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/osm/monaco.osm.pbf"),
        node_store: NodeStore::Memory,
        bounds: None,
        objects: Objects::Every,
    };
    let def = TableDef {
        columns: columns(&tags),
        layers: vec![LayerDef {
            geometry: Some(GeometryType::Line),
            ..LayerDef::new("layer", 0..=14, GRID)
        }],
    };
    let everything = read_all(&OsmSource::new(vec![(def, table)], 16 << 10).unwrap());
    let pushed = source(Some(filter), geometry);
    let expected = taken_by(&pushed, &everything);
    assert!(!expected.is_empty(), "`{filter}` takes no object of Monaco");
    assert_eq!(ids(&read_all(&pushed)), expected, "`{filter}` {geometry:?}");
}

#[test]
fn equality_reads_only_matching_ways() {
    assert_pushdown_exact("highway == 'primary'", None);
}

#[test]
fn not_null_reads_only_tagged_polygons() {
    assert_pushdown_exact("building != null", Some(GeometryType::Polygon));
}

#[test]
fn in_list_reads_only_matching_points() {
    assert_pushdown_exact("amenity in ['cafe', 'restaurant']", Some(GeometryType::Point));
}

#[test]
fn negated_comparison_keeps_null_out() {
    assert_pushdown_exact("highway != 'footway'", Some(GeometryType::Line));
}

#[test]
fn prefix_reads_only_matching_names() {
    assert_pushdown_exact("name.startsWith('Avenue')", None);
}

#[test]
fn or_of_keys_reads_either() {
    assert_pushdown_exact(
        "natural == 'water' || landuse in ['grass', 'forest']",
        Some(GeometryType::Polygon),
    );
}

#[test]
fn has_reads_only_objects_with_the_tag() {
    assert_pushdown_exact("has(feature.name) && !has(feature.highway)", None);
}

#[test]
fn osm_type_selects_nodes() {
    assert_pushdown_exact("osm_type == 'node' && name != null", None);
}

#[test]
fn relations_are_read_as_polygons_only() {
    assert_pushdown_exact("osm_type == 'relation' && landuse != null", Some(GeometryType::Polygon));
}

#[test]
fn untranslatable_where_reads_every_node_and_nothing_else() {
    let everything = read_all(&source(None, None));
    let pushed = source(
        Some("name != null && size(name.split(' ')) > 1"),
        Some(GeometryType::Point),
    );
    let nodes: BTreeSet<(i64, bool)> = ids(&everything).into_iter().filter(|(id, _)| id % 10 == 1).collect();
    assert!(!taken_by(&pushed, &everything).is_empty());
    assert_eq!(ids(&read_all(&pushed)), nodes);
}

#[test]
fn every_tagged_object_is_read_without_a_filter() {
    let features = read_all(&source(None, None));
    let count = |kind| features.iter().filter(|f| f.id.unwrap() % 10 == kind).count();
    insta::assert_snapshot!(
        format!("{} nodes, {} ways, {} relations", count(1), count(2), count(3)),
        @"2427 nodes, 4045 ways, 39 relations"
    );
}

#[test]
fn partition_size_changes_neither_the_features_nor_their_order() {
    let tags: Vec<String> = TAGS.iter().map(|t| (*t).to_owned()).collect();
    let open = |partition_bytes| {
        let table = OsmTable {
            path: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/osm/monaco.osm.pbf"),
            node_store: NodeStore::Memory,
            bounds: None,
            objects: Objects::Taken,
        };
        let def = TableDef {
            columns: columns(&tags),
            layers: vec![LayerDef::new("layer", 0..=14, GRID)],
        };
        OsmSource::new(vec![(def, table)], partition_bytes).unwrap()
    };
    let per_blob = open(1);
    let whole = open(u64::MAX);
    assert!(per_blob.partitions() > whole.partitions() + 3);
    assert_eq!(read_all(&per_blob), read_all(&whole));
}

#[test]
fn every_object_reads_what_no_layer_takes() {
    let tags: Vec<String> = TAGS.iter().map(|t| (*t).to_owned()).collect();
    let table = OsmTable {
        path: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/osm/monaco.osm.pbf"),
        node_store: NodeStore::Memory,
        bounds: None,
        objects: Objects::Every,
    };
    let def = TableDef {
        columns: columns(&tags),
        layers: vec![LayerDef {
            filter: Some("highway == 'primary'".to_owned()),
            geometry: Some(GeometryType::Point),
            ..LayerDef::new("layer", 0..=14, GRID)
        }],
    };
    let every = OsmSource::new(vec![(def, table)], 16 << 10).unwrap();
    assert_eq!(read_all(&every), read_all(&source(None, None)));
}

#[rstest::rstest]
#[case::node(1)]
#[case::way(2)]
#[case::relation(3)]
fn one_object_reads_only_that_object(#[case] kind: i64) {
    let everything = read_all(&source(None, None));
    let wanted = everything.iter().find(|f| f.id.unwrap() % 10 == kind).unwrap();
    let tags: Vec<String> = TAGS.iter().map(|t| (*t).to_owned()).collect();
    let table = OsmTable {
        path: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/osm/monaco.osm.pbf"),
        node_store: NodeStore::Memory,
        bounds: None,
        objects: Objects::One(u64::try_from(wanted.id.unwrap()).unwrap()),
    };
    let def = TableDef {
        columns: columns(&tags),
        layers: vec![LayerDef {
            filter: Some("false".to_owned()),
            ..LayerDef::new("layer", 0..=14, GRID)
        }],
    };
    let one = OsmSource::new(vec![(def, table)], 16 << 10).unwrap();
    assert_eq!(read_all(&one), vec![wanted.clone()]);
}

#[test]
fn one_object_with_an_id_of_no_object_type_is_refused() {
    let tags: Vec<String> = TAGS.iter().map(|t| (*t).to_owned()).collect();
    let table = OsmTable {
        path: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/osm/monaco.osm.pbf"),
        node_store: NodeStore::Memory,
        bounds: None,
        objects: Objects::One(40),
    };
    let def = TableDef {
        columns: columns(&tags),
        layers: vec![LayerDef::new("layer", 0..=14, GRID)],
    };
    let error = OsmSource::new(vec![(def, table)], 16 << 10).unwrap_err();
    insta::assert_snapshot!(error, @"40 is not the feature id of an OSM object, which ends in 1 for a node, 2 for a way and 3 for a relation");
}

#[rstest::rstest]
#[case::points_only(&[GeometryType::Point], 0)]
#[case::lines(&[GeometryType::Point, GeometryType::Line], 1)]
#[case::polygons_twice(&[GeometryType::Polygon, GeometryType::Polygon], 1)]
fn a_shared_store_loads_the_nodes_once_and_only_for_ways(#[case] reads: &[GeometryType], #[case] stores: usize) {
    let temp = tempfile::tempdir().unwrap();
    let node_store = NodeStore::Paged(temp.path().to_path_buf()).shared();
    let tags: Vec<String> = TAGS.iter().map(|t| (*t).to_owned()).collect();
    let sources: Vec<OsmSource> = reads
        .iter()
        .map(|geometry| {
            let table = OsmTable {
                path: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/osm/monaco.osm.pbf"),
                node_store: node_store.clone(),
                bounds: None,
                objects: Objects::Taken,
            };
            let def = TableDef {
                columns: columns(&tags),
                layers: vec![LayerDef {
                    geometry: Some(*geometry),
                    ..LayerDef::new("layer", 0..=14, GRID)
                }],
            };
            OsmSource::new(vec![(def, table)], 16 << 10).unwrap()
        })
        .collect();
    assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), stores);
    assert!(sources.iter().all(|source| !read_all(source).is_empty()));
}
