#![expect(clippy::unwrap_used, clippy::panic)]
#![expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a protobuf encoder reinterprets integers on purpose"
)]

use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::Path;

use geo_types::MultiPolygon;
use indoc::indoc;
use martin_tilegen::LayerGrid;
use martin_tilegen::osm::{NodeStore, OsmSource, OsmTable, columns};
use martin_tilegen::plan::{LayerDef, TableDef};
use martin_tilegen::source::{FeatureSource as _, Geometry};
use wkt::ToWkt as _;

const GRID: LayerGrid = LayerGrid {
    extent: 4096,
    buffer: 64,
};

#[derive(Default)]
struct Pb(Vec<u8>);

impl Pb {
    fn varint(&mut self, mut v: u64) -> &mut Self {
        while v >= 0x80 {
            self.0.push((v as u8) | 0x80);
            v >>= 7;
        }
        self.0.push(v as u8);
        self
    }

    fn uint(&mut self, field: u32, v: u64) -> &mut Self {
        self.varint(u64::from(field << 3)).varint(v)
    }

    fn sint(&mut self, field: u32, v: i64) -> &mut Self {
        self.uint(field, zigzag(v))
    }

    fn bytes(&mut self, field: u32, b: &[u8]) -> &mut Self {
        self.varint(u64::from(field << 3 | 2)).varint(b.len() as u64);
        self.0.extend_from_slice(b);
        self
    }

    fn packed(&mut self, field: u32, values: impl IntoIterator<Item = u64>) -> &mut Self {
        let mut inner = Self::default();
        for v in values {
            inner.varint(v);
        }
        self.bytes(field, &inner.0)
    }

    fn deltas(&mut self, field: u32, values: &[i64]) -> &mut Self {
        let mut last = 0;
        self.packed(
            field,
            values.iter().map(|&v| {
                let delta = v - last;
                last = v;
                zigzag(delta)
            }),
        )
    }
}

fn zigzag(v: i64) -> u64 {
    ((v << 1) ^ (v >> 63)) as u64
}

fn blob(out: &mut Vec<u8>, kind: &str, data: &[u8]) {
    let mut blob = Pb::default();
    blob.bytes(1, data).uint(2, data.len() as u64);
    let mut header = Pb::default();
    header.bytes(1, kind.as_bytes()).uint(3, blob.0.len() as u64);
    out.extend_from_slice(&u32::try_from(header.0.len()).unwrap().to_be_bytes());
    out.extend_from_slice(&header.0);
    out.extend_from_slice(&blob.0);
}

fn field<'a>(words: &[&'a str], prefix: char) -> &'a str {
    words.iter().find_map(|w| w.strip_prefix(prefix)).unwrap_or("")
}

fn list(text: &str) -> impl Iterator<Item = &str> {
    text.split(',').filter(|s| !s.is_empty())
}

fn object(line: &str, strings: &mut Vec<String>, index: &mut HashMap<String, u64>) -> (u32, Pb) {
    let mut sid = |s: &str| {
        *index.entry(s.to_owned()).or_insert_with(|| {
            strings.push(s.to_owned());
            strings.len() as u64 - 1
        })
    };
    let words: Vec<&str> = line.split_whitespace().collect();
    let kind = words[0].chars().next().unwrap();
    let id: i64 = words[0].split_at(1).1.parse().unwrap();
    let (keys, vals): (Vec<u64>, Vec<u64>) = list(field(&words, 'T'))
        .map(|kv| kv.split_once('=').unwrap())
        .map(|(k, v)| (sid(k), sid(v)))
        .unzip();
    let mut pb = Pb::default();
    match kind {
        'n' => {
            let deg = |c| (field(&words, c).parse::<f64>().unwrap() * 1e7).round() as i64;
            pb.sint(1, id)
                .packed(2, keys)
                .packed(3, vals)
                .sint(8, deg('y'))
                .sint(9, deg('x'));
        }
        'w' => {
            let refs: Vec<i64> = list(field(&words, 'N'))
                .map(|n| n.split_at(1).1.parse().unwrap())
                .collect();
            pb.uint(1, id as u64).packed(2, keys).packed(3, vals).deltas(8, &refs);
        }
        'r' => {
            let members: Vec<(u64, i64, u64)> = list(field(&words, 'M'))
                .map(|m| {
                    let (member, role) = m.split_once('@').unwrap();
                    let (kind, id) = member.split_at(1);
                    let kind = match kind {
                        "n" => 0,
                        "w" => 1,
                        _ => 2,
                    };
                    (sid(role), id.parse().unwrap(), kind)
                })
                .collect();
            let ids: Vec<i64> = members.iter().map(|m| m.1).collect();
            pb.uint(1, id as u64)
                .packed(2, keys)
                .packed(3, vals)
                .packed(8, members.iter().map(|m| m.0))
                .deltas(9, &ids)
                .packed(10, members.iter().map(|m| m.2));
        }
        _ => panic!("unknown object {line}"),
    }
    let group_field = match kind {
        'n' => 1,
        'w' => 3,
        _ => 4,
    };
    (group_field, pb)
}

fn block(text: &str) -> Vec<u8> {
    let mut strings = vec![String::new()];
    let mut index = HashMap::new();
    let mut groups: Vec<(u32, Pb)> = Vec::new();
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let (field, pb) = object(line, &mut strings, &mut index);
        if groups.last().is_none_or(|(last, _)| *last != field) {
            groups.push((field, Pb::default()));
        }
        groups.last_mut().unwrap().1.bytes(field, &pb.0);
    }
    let mut table = Pb::default();
    for s in &strings {
        table.bytes(1, s.as_bytes());
    }
    let mut out = Pb::default();
    out.bytes(1, &table.0);
    for (_, group) in &groups {
        out.bytes(2, &group.0);
    }
    out.0
}

fn write_pbf(path: &Path, sorted: bool, opl: &str) {
    let mut header = Pb::default();
    header.bytes(4, b"OsmSchema-V0.6");
    if sorted {
        header.bytes(5, b"Sort.Type_then_ID");
    }
    let mut file = Vec::new();
    blob(&mut file, "OSMHeader", &header.0);
    for text in opl.split("---") {
        blob(&mut file, "OSMData", &block(text));
    }
    std::fs::write(path, file).unwrap();
}

fn wkt(geometry: Geometry) -> String {
    match geometry {
        Geometry::Point(c) => geo_types::Point(c).wkt_string(),
        Geometry::Line(l) => l.wkt_string(),
        Geometry::Polygon(p) => p.wkt_string(),
        Geometry::Polygons(ps) => MultiPolygon(ps).wkt_string(),
        other @ (Geometry::Points(_) | Geometry::Lines(_) | Geometry::Wkb(_)) => format!("{other:?}"),
    }
}

fn read_with(path: &Path, node_store: NodeStore) -> String {
    let table = OsmTable {
        path: path.to_path_buf(),
        node_store,
        bounds: None,
    };
    let def = TableDef {
        columns: columns(&["name".to_owned()]),
        layers: vec![LayerDef::new("layer", 0..=14, GRID)],
    };
    let source = match OsmSource::new(vec![(def, table)], 1) {
        Ok(source) => source,
        Err(error) => return format!("{error}").replace(&path.display().to_string(), "test.osm.pbf"),
    };
    let mut out = String::new();
    for partition in 0..source.partitions() {
        source
            .read(partition, source.plan().table_keys(), &mut |batch| {
                for feature in batch.features {
                    let id = feature.id.unwrap();
                    let kind = ["", "n", "w", "r"][(id % 10) as usize];
                    writeln!(out, "{kind}{} {}", id / 10, wkt(feature.geometry)).unwrap();
                }
                Ok(())
            })
            .unwrap();
    }
    out
}

fn features(sorted: bool, opl: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.osm.pbf");
    write_pbf(&path, sorted, opl);
    let memory = read_with(&path, NodeStore::Memory);
    let paged = read_with(&path, NodeStore::Paged(dir.path().to_path_buf()));
    assert_eq!(memory, paged);
    memory
}

#[test]
fn way_between_the_blocks_of_its_nodes() {
    let opl = indoc! {"
        n1 x0 y0
        n2 x1 y0
        ---
        w10 Tname=road Nn1,n2,n3,n4
        ---
        n3 x1 y1
        n4 x0 y1
    "};
    insta::assert_snapshot!(features(false, opl), @"w10 LINESTRING(0 0,1 0,1 1,0 1)");
}

#[test]
fn way_between_the_blocks_of_its_nodes_in_a_file_declared_sorted() {
    let opl = indoc! {"
        n1 x0 y0
        n2 x1 y0
        ---
        w10 Tname=road Nn1,n2,n3,n4
        ---
        n3 x1 y1
        n4 x0 y1
    "};
    insta::assert_snapshot!(features(true, opl), @"w10 LINESTRING(0 0,1 0,1 1,0 1)");
}

#[test]
fn way_before_its_nodes_in_one_block() {
    let opl = indoc! {"
        w10 Tname=road Nn1,n2
        n1 x0 y0
        n2 x1 y0
    "};
    insta::assert_snapshot!(features(false, opl), @"w10 LINESTRING(0 0,1 0)");
}

#[test]
fn relations_then_ways_then_nodes() {
    let opl = indoc! {"
        r100 Ttype=multipolygon,name=area Mw10@outer
        ---
        w10 Nn1,n2,n3,n1
        ---
        n1 x0 y0
        n2 x1 y0
        n3 x1 y1
    "};
    insta::assert_snapshot!(features(false, opl), @"r100 POLYGON((1 1,0 0,1 0,1 1))");
}

#[test]
fn node_ids_out_of_order_fail_the_read() {
    let opl = indoc! {"
        n3 x1 y1
        n4 x0 y1
        ---
        w10 Tname=road Nn1,n2,n3,n4
        ---
        n1 x0 y0
        n2 x1 y0
    "};
    insta::assert_snapshot!(features(false, opl), @"Cannot read test.osm.pbf: node IDs must be loaded in ascending order: 1 after 4");
}

#[test]
fn node_twice_fails_the_read() {
    let opl = indoc! {"
        n1 x0 y0 Tname=a
        ---
        n1 x1 y1 Tname=b
    "};
    insta::assert_snapshot!(features(false, opl), @"Cannot read test.osm.pbf: node IDs must be loaded in ascending order: 1 after 1");
}

#[test]
fn relation_that_is_its_own_member() {
    let opl = indoc! {"
        n1 x0 y0
        n2 x1 y0
        n3 x1 y1
        ---
        w10 Nn1,n2,n3,n1
        ---
        r100 Ttype=multipolygon,name=self Mw10@outer,r100@
    "};
    insta::assert_snapshot!(features(true, opl), @"r100 POLYGON((1 1,0 0,1 0,1 1))");
}

#[test]
fn relations_that_are_members_of_each_other() {
    let opl = indoc! {"
        n1 x0 y0
        n2 x1 y0
        n3 x1 y1
        n4 x0 y1
        ---
        w10 Nn1,n2,n3,n1
        w11 Nn1,n3,n4,n1
        ---
        r100 Ttype=multipolygon,name=a Mw10@outer,r101@
        r101 Ttype=multipolygon,name=b Mw11@outer,r100@
    "};
    insta::assert_snapshot!(features(true, opl), @"
    r100 POLYGON((1 1,0 0,1 0,1 1))
    r101 POLYGON((0 1,0 0,1 1,0 1))
    ");
}

#[test]
fn boundary_of_only_subareas_is_left_out() {
    let opl = indoc! {"
        n1 x0 y0
        n2 x1 y0
        n3 x1 y1
        ---
        w10 Nn1,n2,n3,n1
        ---
        r100 Ttype=boundary,name=sub Mw10@outer
        r101 Ttype=boundary,name=super Mr100@subarea,r101@subarea
    "};
    insta::assert_snapshot!(features(true, opl), @"r100 POLYGON((1 1,0 0,1 0,1 1))");
}

#[test]
fn closed_way_listed_twice_counts_once() {
    let opl = indoc! {"
        n1 x0 y0
        n2 x1 y0
        n3 x1 y1
        ---
        w10 Nn1,n2,n3,n1
        ---
        r100 Ttype=multipolygon,name=twice Mw10@outer,w10@outer
    "};
    insta::assert_snapshot!(features(true, opl), @"r100 POLYGON((1 1,0 0,1 0,1 1))");
}

#[test]
fn open_way_listed_twice_counts_once() {
    let opl = indoc! {"
        n1 x0 y0
        n2 x1 y0
        n3 x1 y1
        ---
        w10 Nn1,n2,n3
        w11 Nn3,n1
        ---
        r100 Ttype=multipolygon,name=twice Mw10@outer,w11@outer,w10@outer
    "};
    insta::assert_snapshot!(features(true, opl), @"r100 POLYGON((1 1,0 0,1 0,1 1))");
}

#[test]
fn ring_of_open_ways_in_any_order_and_direction() {
    let opl = indoc! {"
        n1 x0 y0
        n2 x1 y0
        n3 x1 y1
        n4 x0 y1
        ---
        w10 Nn1,n2
        w11 Nn3,n2
        w12 Nn3,n4
        w13 Nn1,n4
        ---
        r100 Ttype=multipolygon,name=rev Mw11@outer,w13@outer,w10@outer,w12@outer
    "};
    insta::assert_snapshot!(features(true, opl), @"r100 POLYGON((0 1,0 0,1 0,1 1,0 1))");
}

#[test]
fn rings_touching_at_a_node_become_two_polygons() {
    let opl = indoc! {"
        n1 x0 y0
        n2 x1 y0
        n3 x1 y1
        n4 x2 y1
        n5 x2 y2
        ---
        w10 Nn1,n2,n3
        w11 Nn3,n1
        w12 Nn3,n4,n5
        w13 Nn5,n3
        ---
        r100 Ttype=multipolygon,name=eight Mw10@outer,w12@outer,w11@outer,w13@outer
    "};
    insta::assert_snapshot!(features(true, opl), @"r100 MULTIPOLYGON(((1 1,0 0,1 0,1 1)),((2 2,1 1,2 1,2 2)))");
}

#[test]
fn inner_ring_is_a_hole() {
    let opl = indoc! {"
        n1 x0 y0
        n2 x4 y0
        n3 x4 y4
        n4 x0 y4
        n5 x1 y1
        n6 x2 y1
        n7 x2 y2
        ---
        w10 Nn1,n2,n3,n4,n1
        w11 Nn5,n6,n7,n5
        ---
        r100 Ttype=multipolygon,name=hole Mw11@inner,w10@outer
    "};
    insta::assert_snapshot!(features(true, opl), @"r100 POLYGON((0 4,0 0,4 0,4 4,0 4),(1 1,2 2,2 1,1 1))");
}

#[test]
fn relation_with_a_way_the_file_lacks_is_left_out() {
    let opl = indoc! {"
        n1 x0 y0
        n2 x1 y0
        n3 x1 y1
        ---
        w10 Nn1,n2,n3,n1
        ---
        r100 Ttype=multipolygon,name=partial Mw10@outer,w99@outer
        r101 Ttype=multipolygon,name=whole Mw10@outer
    "};
    insta::assert_snapshot!(features(true, opl), @"r101 POLYGON((1 1,0 0,1 0,1 1))");
}

#[test]
fn relation_with_a_node_the_file_lacks_is_left_out() {
    let opl = indoc! {"
        n1 x0 y0
        n2 x1 y0
        n3 x1 y1
        ---
        w10 Nn1,n2,n99,n1
        w11 Nn1,n2,n3,n1
        ---
        r100 Ttype=multipolygon,name=lacks Mw10@outer
        r101 Ttype=multipolygon,name=whole Mw11@outer
    "};
    insta::assert_snapshot!(features(true, opl), @"r101 POLYGON((1 1,0 0,1 0,1 1))");
}

#[test]
fn relation_whose_ways_do_not_close_is_left_out() {
    let opl = indoc! {"
        n1 x0 y0
        n2 x1 y0
        n3 x1 y1
        ---
        w10 Nn1,n2,n3
        w11 Nn1,n2,n3,n1
        ---
        r100 Ttype=multipolygon,name=open Mw10@outer
        r101 Ttype=multipolygon,name=whole Mw11@outer
    "};
    insta::assert_snapshot!(features(true, opl), @"r101 POLYGON((1 1,0 0,1 0,1 1))");
}

#[test]
fn way_with_a_node_the_file_lacks_is_left_out() {
    let opl = indoc! {"
        n1 x0 y0
        n2 x1 y0
        ---
        w10 Tname=road Nn1,n99
        w11 Tname=road Nn1,n2
    "};
    insta::assert_snapshot!(features(true, opl), @"w11 LINESTRING(0 0,1 0)");
}

#[test]
fn way_of_one_node_is_left_out() {
    let opl = indoc! {"
        n1 x0 y0
        n2 x1 y0
        ---
        w10 Tname=road Nn1
        w11 Tname=road Nn1,n2
    "};
    insta::assert_snapshot!(features(true, opl), @"w11 LINESTRING(0 0,1 0)");
}

#[test]
fn way_that_revisits_its_nodes_keeps_them() {
    let opl = indoc! {"
        n1 x0 y0
        n2 x1 y0
        n3 x1 y1
        ---
        w10 Tname=road Nn1,n2,n1,n2,n3
    "};
    insta::assert_snapshot!(features(true, opl), @"w10 LINESTRING(0 0,1 0,0 0,1 0,1 1)");
}
