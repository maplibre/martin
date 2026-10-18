#![cfg(all(feature = "test-pg", feature = "unstable-generate"))]
#![expect(clippy::panic, clippy::unwrap_used)]

use std::collections::BTreeSet;

use indoc::indoc;
use martin::config::file::postgres::TableInfo;
use martin::config::file::postgres::resolver::scan_sql;
use martin::generate::layers::{LowerOptions, lower_table};
use martin_core::tiles::postgres::{
    PooledConnection, PostgresPool, RetryTimeout, ScanLayout, scan_features,
};
use martin_tilegen::FeatureGeom;
use martin_tilegen::plan::{Plan, RowFilter};
use martin_tilegen::source::Geometry;
use rstest::rstest;

const ROWS: &str = indoc! {"
    CREATE TEMP TABLE where_rows (
      id int8, geom geometry(Geometry, 4326), b bool, i int4, f float8, s text, d date, n int8
    );
    INSERT INTO where_rows VALUES
      (1, 'SRID=4326;POINT(0 0)', true, 1, 1.5, 'abc', '2020-01-01', 9007199254740992),
      (2, 'SRID=4326;LINESTRING(0 0, 1 1)', false, -3, 'NaN', 'a%_c', NULL, NULL),
      (3, 'SRID=4326;POLYGON((0 0, 1 0, 1 1, 0 0))', NULL, NULL, NULL, NULL, NULL, NULL),
      (4, 'SRID=4326;MULTIPOINT(0 0, 1 1)', true, 0, -0.0, 'ABC', '1999-12-31', NULL),
      (5, 'SRID=4326;POINT(1 1)', false, 2147483647, 'Infinity', 'Übel', NULL, NULL),
      (6, 'SRID=4326;MULTIPOLYGON(((0 0, 1 0, 1 1, 0 0)))', NULL, 3, 2, '', '2020-01-01', NULL),
      (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', true, 2, '-Infinity', 'a\\b', NULL, NULL),
      (8, 'SRID=4326;POINT(2 2)', false, -1, 3.25, 'abd', NULL, NULL);
"};

async fn connect() -> PooledConnection {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL");
    let pool = PostgresPool::new(&url, None, None, None, 1, RetryTimeout::default())
        .await
        .unwrap();
    let client = pool.get().await.unwrap();
    client.batch_execute(ROWS).await.unwrap();
    client
}

/// The ids of the rows the SQL condition keeps, and of those the plan's layer takes.
async fn kept(client: &PooledConnection, yaml: &str) -> (BTreeSet<u64>, BTreeSet<u64>) {
    let options = LowerOptions {
        zooms: 0..=0,
        bounds: None,
    };
    let info: TableInfo = serde_saphyr::from_str(yaml).unwrap();
    let scan = lower_table("where_rows", info, &options).unwrap().unwrap();
    let info = scan.info;
    let plan = Plan::new(vec![scan.def]).unwrap();
    let rows = plan.row_filter(0, 0..=0);
    if let RowFilter::Untranslatable(err) = &rows {
        panic!("{err}");
    }
    let filtered = scan_sql(&info, plan.columns(0), rows.condition(), None).unwrap();
    let in_sql = client
        .query(&filtered.sql, &[])
        .await
        .unwrap()
        .iter()
        .map(|row| u64::try_from(row.get::<_, i64>(1)).unwrap())
        .collect();

    let all = scan_sql(&info, plan.columns(0), None, None).unwrap();
    let layout = ScanLayout {
        table: 0,
        crs: all.crs,
        has_id: true,
        properties: all.properties.len(),
    };
    let mut features = Vec::new();
    scan_features(
        client,
        &all.sql,
        &[],
        &layout,
        0,
        &plan.table_keys()[0],
        |batch| {
            features.extend(batch.features);
            Ok(())
        },
    )
    .await
    .unwrap();
    let mut evaluator = plan.evaluator();
    let mut in_plan = BTreeSet::new();
    for mut feature in features {
        assert!(feature.geometry.decode_wkb());
        let geom = match &feature.geometry {
            Geometry::Points(points) => FeatureGeom::Points(points),
            Geometry::Lines(lines) => FeatureGeom::Lines(lines),
            Geometry::Polygons(polygons) => FeatureGeom::Polygons(polygons),
            Geometry::Wkb(_) => panic!("undecoded geometry"),
        };
        evaluator
            .evaluate(0, &feature, geom, |_| {
                in_plan.insert(feature.id.unwrap());
                Ok(())
            })
            .unwrap();
    }
    (in_sql, in_plan)
}

#[rstest]
#[case::bool_column(indoc! {r"
    schema: pg_temp
    table: where_rows
    srid: 4326
    geometry_column: geom
    geometry_type: GEOMETRY
    id_column: id
    properties:
      b: bool
      i: int4
      f: float8
      s: text
      d: date
      n: int8
    layers:
      l:
        where: b
"}, &[1, 4, 7])]
#[case::negated_bool_column(indoc! {r"
    schema: pg_temp
    table: where_rows
    srid: 4326
    geometry_column: geom
    geometry_type: GEOMETRY
    id_column: id
    properties:
      b: bool
      i: int4
      f: float8
      s: text
      d: date
      n: int8
    layers:
      l:
        where: '!b'
"}, &[2, 5, 8])]
#[case::bool_equals_true(indoc! {r"
    schema: pg_temp
    table: where_rows
    srid: 4326
    geometry_column: geom
    geometry_type: GEOMETRY
    id_column: id
    properties:
      b: bool
      i: int4
      f: float8
      s: text
      d: date
      n: int8
    layers:
      l:
        where: 'b == true'
"}, &[1, 4, 7])]
#[case::bool_not_equals_true(indoc! {r"
    schema: pg_temp
    table: where_rows
    srid: 4326
    geometry_column: geom
    geometry_type: GEOMETRY
    id_column: id
    properties:
      b: bool
      i: int4
      f: float8
      s: text
      d: date
      n: int8
    layers:
      l:
        where: 'b != true'
"}, &[2, 3, 5, 6, 8])]
#[case::bool_equals_null(indoc! {r"
    schema: pg_temp
    table: where_rows
    srid: 4326
    geometry_column: geom
    geometry_type: GEOMETRY
    id_column: id
    properties:
      b: bool
      i: int4
      f: float8
      s: text
      d: date
      n: int8
    layers:
      l:
        where: 'b == null'
"}, &[3, 6])]
#[case::bool_not_equals_null(indoc! {r"
    schema: pg_temp
    table: where_rows
    srid: 4326
    geometry_column: geom
    geometry_type: GEOMETRY
    id_column: id
    properties:
      b: bool
      i: int4
      f: float8
      s: text
      d: date
      n: int8
    layers:
      l:
        where: 'b != null'
"}, &[1, 2, 4, 5, 7, 8])]
#[case::int_greater(indoc! {r"
    schema: pg_temp
    table: where_rows
    srid: 4326
    geometry_column: geom
    geometry_type: GEOMETRY
    id_column: id
    properties:
      b: bool
      i: int4
      f: float8
      s: text
      d: date
      n: int8
    layers:
      l:
        where: 'i > 1'
"}, &[5, 6, 7])]
#[case::negated_int_greater(indoc! {r"
    schema: pg_temp
    table: where_rows
    srid: 4326
    geometry_column: geom
    geometry_type: GEOMETRY
    id_column: id
    properties:
      b: bool
      i: int4
      f: float8
      s: text
      d: date
      n: int8
    layers:
      l:
        where: '!(i > 1)'
"}, &[1, 2, 4, 8])]
#[case::int_at_most_float(indoc! {r"
    schema: pg_temp
    table: where_rows
    srid: 4326
    geometry_column: geom
    geometry_type: GEOMETRY
    id_column: id
    properties:
      b: bool
      i: int4
      f: float8
      s: text
      d: date
      n: int8
    layers:
      l:
        where: 'i <= 1.5'
"}, &[1, 2, 4, 8])]
#[case::int_equals(indoc! {r"
    schema: pg_temp
    table: where_rows
    srid: 4326
    geometry_column: geom
    geometry_type: GEOMETRY
    id_column: id
    properties:
      b: bool
      i: int4
      f: float8
      s: text
      d: date
      n: int8
    layers:
      l:
        where: 'i == 3'
"}, &[6])]
#[case::int_not_equals(indoc! {r"
    schema: pg_temp
    table: where_rows
    srid: 4326
    geometry_column: geom
    geometry_type: GEOMETRY
    id_column: id
    properties:
      b: bool
      i: int4
      f: float8
      s: text
      d: date
      n: int8
    layers:
      l:
        where: 'i != 3'
"}, &[1, 2, 3, 4, 5, 7, 8])]
#[case::negative_literal_less_than_int(indoc! {r"
    schema: pg_temp
    table: where_rows
    srid: 4326
    geometry_column: geom
    geometry_type: GEOMETRY
    id_column: id
    properties:
      b: bool
      i: int4
      f: float8
      s: text
      d: date
      n: int8
    layers:
      l:
        where: '-1 < i'
"}, &[1, 4, 5, 6, 7])]
#[case::int_in_list_with_null(indoc! {r"
    schema: pg_temp
    table: where_rows
    srid: 4326
    geometry_column: geom
    geometry_type: GEOMETRY
    id_column: id
    properties:
      b: bool
      i: int4
      f: float8
      s: text
      d: date
      n: int8
    layers:
      l:
        where: 'i in [1, 2, null]'
"}, &[1, 3, 7])]
#[case::int_not_in_list(indoc! {r"
    schema: pg_temp
    table: where_rows
    srid: 4326
    geometry_column: geom
    geometry_type: GEOMETRY
    id_column: id
    properties:
      b: bool
      i: int4
      f: float8
      s: text
      d: date
      n: int8
    layers:
      l:
        where: '!(i in [1, 2])'
"}, &[2, 3, 4, 5, 6, 8])]
#[case::int_not_in_list_with_null(indoc! {r"
    schema: pg_temp
    table: where_rows
    srid: 4326
    geometry_column: geom
    geometry_type: GEOMETRY
    id_column: id
    properties:
      b: bool
      i: int4
      f: float8
      s: text
      d: date
      n: int8
    layers:
      l:
        where: '!(i in [1, null])'
"}, &[2, 4, 5, 6, 7, 8])]
#[case::int_in_empty_list(indoc! {r"
    schema: pg_temp
    table: where_rows
    srid: 4326
    geometry_column: geom
    geometry_type: GEOMETRY
    id_column: id
    properties:
      b: bool
      i: int4
      f: float8
      s: text
      d: date
      n: int8
    layers:
      l:
        where: 'i in []'
"}, &[])]
#[case::int_not_in_empty_list(indoc! {r"
    schema: pg_temp
    table: where_rows
    srid: 4326
    geometry_column: geom
    geometry_type: GEOMETRY
    id_column: id
    properties:
      b: bool
      i: int4
      f: float8
      s: text
      d: date
      n: int8
    layers:
      l:
        where: '!(i in [])'
"}, &[1, 2, 3, 4, 5, 6, 7, 8])]
#[case::float_greater(indoc! {r"
    schema: pg_temp
    table: where_rows
    srid: 4326
    geometry_column: geom
    geometry_type: GEOMETRY
    id_column: id
    properties:
      b: bool
      i: int4
      f: float8
      s: text
      d: date
      n: int8
    layers:
      l:
        where: 'f > 1.0'
"}, &[1, 5, 6, 8])]
#[case::negated_float_greater(indoc! {r"
    schema: pg_temp
    table: where_rows
    srid: 4326
    geometry_column: geom
    geometry_type: GEOMETRY
    id_column: id
    properties:
      b: bool
      i: int4
      f: float8
      s: text
      d: date
      n: int8
    layers:
      l:
        where: '!(f > 1.0)'
"}, &[4, 7])]
#[case::float_equals_int(indoc! {r"
    schema: pg_temp
    table: where_rows
    srid: 4326
    geometry_column: geom
    geometry_type: GEOMETRY
    id_column: id
    properties:
      b: bool
      i: int4
      f: float8
      s: text
      d: date
      n: int8
    layers:
      l:
        where: 'f == 2'
"}, &[6])]
#[case::float_not_equals_int(indoc! {r"
    schema: pg_temp
    table: where_rows
    srid: 4326
    geometry_column: geom
    geometry_type: GEOMETRY
    id_column: id
    properties:
      b: bool
      i: int4
      f: float8
      s: text
      d: date
      n: int8
    layers:
      l:
        where: 'f != 2'
"}, &[1, 2, 3, 4, 5, 7, 8])]
#[case::float_less_than_int(indoc! {r"
    schema: pg_temp
    table: where_rows
    srid: 4326
    geometry_column: geom
    geometry_type: GEOMETRY
    id_column: id
    properties:
      b: bool
      i: int4
      f: float8
      s: text
      d: date
      n: int8
    layers:
      l:
        where: 'f < 0'
"}, &[7])]
#[case::negated_float_less_than_int(indoc! {r"
    schema: pg_temp
    table: where_rows
    srid: 4326
    geometry_column: geom
    geometry_type: GEOMETRY
    id_column: id
    properties:
      b: bool
      i: int4
      f: float8
      s: text
      d: date
      n: int8
    layers:
      l:
        where: '!(f < 0)'
"}, &[1, 4, 5, 6, 8])]
#[case::float_in_list(indoc! {r"
    schema: pg_temp
    table: where_rows
    srid: 4326
    geometry_column: geom
    geometry_type: GEOMETRY
    id_column: id
    properties:
      b: bool
      i: int4
      f: float8
      s: text
      d: date
      n: int8
    layers:
      l:
        where: 'f in [1.5, 2]'
"}, &[1, 6])]
#[case::float_not_in_list(indoc! {r"
    schema: pg_temp
    table: where_rows
    srid: 4326
    geometry_column: geom
    geometry_type: GEOMETRY
    id_column: id
    properties:
      b: bool
      i: int4
      f: float8
      s: text
      d: date
      n: int8
    layers:
      l:
        where: '!(f in [1.5, 2])'
"}, &[2, 3, 4, 5, 7, 8])]
#[case::int_equals_float_column(indoc! {r"
    schema: pg_temp
    table: where_rows
    srid: 4326
    geometry_column: geom
    geometry_type: GEOMETRY
    id_column: id
    properties:
      b: bool
      i: int4
      f: float8
      s: text
      d: date
      n: int8
    layers:
      l:
        where: 'i == f'
"}, &[3, 4])]
#[case::int_not_equals_float_column(indoc! {r"
    schema: pg_temp
    table: where_rows
    srid: 4326
    geometry_column: geom
    geometry_type: GEOMETRY
    id_column: id
    properties:
      b: bool
      i: int4
      f: float8
      s: text
      d: date
      n: int8
    layers:
      l:
        where: 'i != f'
"}, &[1, 2, 5, 6, 7, 8])]
#[case::int8_in_list_of_float_and_int(indoc! {r"
    schema: pg_temp
    table: where_rows
    srid: 4326
    geometry_column: geom
    geometry_type: GEOMETRY
    id_column: id
    properties:
      b: bool
      i: int4
      f: float8
      s: text
      d: date
      n: int8
    layers:
      l:
        where: 'n in [0.5, 9007199254740993]'
"}, &[])]
#[case::int8_in_list_of_int(indoc! {r"
    schema: pg_temp
    table: where_rows
    srid: 4326
    geometry_column: geom
    geometry_type: GEOMETRY
    id_column: id
    properties:
      b: bool
      i: int4
      f: float8
      s: text
      d: date
      n: int8
    layers:
      l:
        where: 'n in [9007199254740992]'
"}, &[1])]
#[case::string_equals(indoc! {r#"
    schema: pg_temp
    table: where_rows
    srid: 4326
    geometry_column: geom
    geometry_type: GEOMETRY
    id_column: id
    properties:
      b: bool
      i: int4
      f: float8
      s: text
      d: date
      n: int8
    layers:
      l:
        where: "s == 'abc'"
"#}, &[1])]
#[case::string_not_equals(indoc! {r#"
    schema: pg_temp
    table: where_rows
    srid: 4326
    geometry_column: geom
    geometry_type: GEOMETRY
    id_column: id
    properties:
      b: bool
      i: int4
      f: float8
      s: text
      d: date
      n: int8
    layers:
      l:
        where: "s != 'abc'"
"#}, &[2, 3, 4, 5, 6, 7, 8])]
#[case::string_less(indoc! {r#"
    schema: pg_temp
    table: where_rows
    srid: 4326
    geometry_column: geom
    geometry_type: GEOMETRY
    id_column: id
    properties:
      b: bool
      i: int4
      f: float8
      s: text
      d: date
      n: int8
    layers:
      l:
        where: "s < 'abd'"
"#}, &[1, 2, 4, 6, 7])]
#[case::negated_string_less(indoc! {r#"
    schema: pg_temp
    table: where_rows
    srid: 4326
    geometry_column: geom
    geometry_type: GEOMETRY
    id_column: id
    properties:
      b: bool
      i: int4
      f: float8
      s: text
      d: date
      n: int8
    layers:
      l:
        where: "!(s < 'abd')"
"#}, &[5, 8])]
#[case::string_at_least(indoc! {r#"
    schema: pg_temp
    table: where_rows
    srid: 4326
    geometry_column: geom
    geometry_type: GEOMETRY
    id_column: id
    properties:
      b: bool
      i: int4
      f: float8
      s: text
      d: date
      n: int8
    layers:
      l:
        where: "s >= 'a'"
"#}, &[1, 2, 5, 7, 8])]
#[case::starts_with_percent(indoc! {r#"
    schema: pg_temp
    table: where_rows
    srid: 4326
    geometry_column: geom
    geometry_type: GEOMETRY
    id_column: id
    properties:
      b: bool
      i: int4
      f: float8
      s: text
      d: date
      n: int8
    layers:
      l:
        where: "s.startsWith('a%')"
"#}, &[2])]
#[case::ends_with(indoc! {r#"
    schema: pg_temp
    table: where_rows
    srid: 4326
    geometry_column: geom
    geometry_type: GEOMETRY
    id_column: id
    properties:
      b: bool
      i: int4
      f: float8
      s: text
      d: date
      n: int8
    layers:
      l:
        where: "s.endsWith('c')"
"#}, &[1, 2])]
#[case::contains_underscore(indoc! {r#"
    schema: pg_temp
    table: where_rows
    srid: 4326
    geometry_column: geom
    geometry_type: GEOMETRY
    id_column: id
    properties:
      b: bool
      i: int4
      f: float8
      s: text
      d: date
      n: int8
    layers:
      l:
        where: "s.contains('_')"
"#}, &[2])]
#[case::contains_backslash(indoc! {r#"
    schema: pg_temp
    table: where_rows
    srid: 4326
    geometry_column: geom
    geometry_type: GEOMETRY
    id_column: id
    properties:
      b: bool
      i: int4
      f: float8
      s: text
      d: date
      n: int8
    layers:
      l:
        where: "s.contains('\\\\')"
"#}, &[7])]
#[case::negated_starts_with(indoc! {r#"
    schema: pg_temp
    table: where_rows
    srid: 4326
    geometry_column: geom
    geometry_type: GEOMETRY
    id_column: id
    properties:
      b: bool
      i: int4
      f: float8
      s: text
      d: date
      n: int8
    layers:
      l:
        where: "!s.startsWith('a')"
"#}, &[4, 5, 6])]
#[case::size_equals(indoc! {r"
    schema: pg_temp
    table: where_rows
    srid: 4326
    geometry_column: geom
    geometry_type: GEOMETRY
    id_column: id
    properties:
      b: bool
      i: int4
      f: float8
      s: text
      d: date
      n: int8
    layers:
      l:
        where: 'size(s) == 3'
"}, &[1, 4, 7, 8])]
#[case::size_not_equals(indoc! {r"
    schema: pg_temp
    table: where_rows
    srid: 4326
    geometry_column: geom
    geometry_type: GEOMETRY
    id_column: id
    properties:
      b: bool
      i: int4
      f: float8
      s: text
      d: date
      n: int8
    layers:
      l:
        where: 'size(s) != 3'
"}, &[2, 5, 6])]
#[case::size_greater(indoc! {r"
    schema: pg_temp
    table: where_rows
    srid: 4326
    geometry_column: geom
    geometry_type: GEOMETRY
    id_column: id
    properties:
      b: bool
      i: int4
      f: float8
      s: text
      d: date
      n: int8
    layers:
      l:
        where: 'size(s) > 2'
"}, &[1, 2, 4, 5, 7, 8])]
#[case::lower_ascii_equals(indoc! {r#"
    schema: pg_temp
    table: where_rows
    srid: 4326
    geometry_column: geom
    geometry_type: GEOMETRY
    id_column: id
    properties:
      b: bool
      i: int4
      f: float8
      s: text
      d: date
      n: int8
    layers:
      l:
        where: "s.lowerAscii() == 'abc'"
"#}, &[1, 4])]
#[case::upper_ascii_equals(indoc! {r#"
    schema: pg_temp
    table: where_rows
    srid: 4326
    geometry_column: geom
    geometry_type: GEOMETRY
    id_column: id
    properties:
      b: bool
      i: int4
      f: float8
      s: text
      d: date
      n: int8
    layers:
      l:
        where: "s.upperAscii() == 'ÜBEL'"
"#}, &[5])]
#[case::string_in_list(indoc! {r#"
    schema: pg_temp
    table: where_rows
    srid: 4326
    geometry_column: geom
    geometry_type: GEOMETRY
    id_column: id
    properties:
      b: bool
      i: int4
      f: float8
      s: text
      d: date
      n: int8
    layers:
      l:
        where: "s in ['abc', 'ABC']"
"#}, &[1, 4])]
#[case::string_not_in_list(indoc! {r#"
    schema: pg_temp
    table: where_rows
    srid: 4326
    geometry_column: geom
    geometry_type: GEOMETRY
    id_column: id
    properties:
      b: bool
      i: int4
      f: float8
      s: text
      d: date
      n: int8
    layers:
      l:
        where: "!(s in ['abc', 'ABC'])"
"#}, &[2, 3, 5, 6, 7, 8])]
#[case::has_field(indoc! {r"
    schema: pg_temp
    table: where_rows
    srid: 4326
    geometry_column: geom
    geometry_type: GEOMETRY
    id_column: id
    properties:
      b: bool
      i: int4
      f: float8
      s: text
      d: date
      n: int8
    layers:
      l:
        where: 'has(feature.s)'
"}, &[1, 2, 4, 5, 6, 7, 8])]
#[case::negated_has_field(indoc! {r"
    schema: pg_temp
    table: where_rows
    srid: 4326
    geometry_column: geom
    geometry_type: GEOMETRY
    id_column: id
    properties:
      b: bool
      i: int4
      f: float8
      s: text
      d: date
      n: int8
    layers:
      l:
        where: '!has(feature.s)'
"}, &[3])]
#[case::key_in_feature(indoc! {r#"
    schema: pg_temp
    table: where_rows
    srid: 4326
    geometry_column: geom
    geometry_type: GEOMETRY
    id_column: id
    properties:
      b: bool
      i: int4
      f: float8
      s: text
      d: date
      n: int8
    layers:
      l:
        where: "'d' in feature"
"#}, &[1, 4, 6])]
#[case::feature_index_equals(indoc! {r#"
    schema: pg_temp
    table: where_rows
    srid: 4326
    geometry_column: geom
    geometry_type: GEOMETRY
    id_column: id
    properties:
      b: bool
      i: int4
      f: float8
      s: text
      d: date
      n: int8
    layers:
      l:
        where: "feature['s'] == 'abc'"
"#}, &[1])]
#[case::feature_field_equals(indoc! {r#"
    schema: pg_temp
    table: where_rows
    srid: 4326
    geometry_column: geom
    geometry_type: GEOMETRY
    id_column: id
    properties:
      b: bool
      i: int4
      f: float8
      s: text
      d: date
      n: int8
    layers:
      l:
        where: "feature.s == 'abc'"
"#}, &[1])]
#[case::date_equals(indoc! {r#"
    schema: pg_temp
    table: where_rows
    srid: 4326
    geometry_column: geom
    geometry_type: GEOMETRY
    id_column: id
    properties:
      b: bool
      i: int4
      f: float8
      s: text
      d: date
      n: int8
    layers:
      l:
        where: "d == '2020-01-01'"
"#}, &[1, 6])]
#[case::date_greater(indoc! {r#"
    schema: pg_temp
    table: where_rows
    srid: 4326
    geometry_column: geom
    geometry_type: GEOMETRY
    id_column: id
    properties:
      b: bool
      i: int4
      f: float8
      s: text
      d: date
      n: int8
    layers:
      l:
        where: "d > '2000'"
"#}, &[1, 6])]
#[case::string_equals_int(indoc! {r"
    schema: pg_temp
    table: where_rows
    srid: 4326
    geometry_column: geom
    geometry_type: GEOMETRY
    id_column: id
    properties:
      b: bool
      i: int4
      f: float8
      s: text
      d: date
      n: int8
    layers:
      l:
        where: 's == 1'
"}, &[])]
#[case::string_not_equals_int(indoc! {r"
    schema: pg_temp
    table: where_rows
    srid: 4326
    geometry_column: geom
    geometry_type: GEOMETRY
    id_column: id
    properties:
      b: bool
      i: int4
      f: float8
      s: text
      d: date
      n: int8
    layers:
      l:
        where: 's != 1'
"}, &[1, 2, 3, 4, 5, 6, 7, 8])]
#[case::int_equals_string(indoc! {r#"
    schema: pg_temp
    table: where_rows
    srid: 4326
    geometry_column: geom
    geometry_type: GEOMETRY
    id_column: id
    properties:
      b: bool
      i: int4
      f: float8
      s: text
      d: date
      n: int8
    layers:
      l:
        where: "i == 'x'"
"#}, &[])]
#[case::string_greater_than_int(indoc! {r"
    schema: pg_temp
    table: where_rows
    srid: 4326
    geometry_column: geom
    geometry_type: GEOMETRY
    id_column: id
    properties:
      b: bool
      i: int4
      f: float8
      s: text
      d: date
      n: int8
    layers:
      l:
        where: 's > 1'
"}, &[])]
#[case::negated_string_greater_than_int(indoc! {r"
    schema: pg_temp
    table: where_rows
    srid: 4326
    geometry_column: geom
    geometry_type: GEOMETRY
    id_column: id
    properties:
      b: bool
      i: int4
      f: float8
      s: text
      d: date
      n: int8
    layers:
      l:
        where: '!(s > 1)'
"}, &[])]
#[case::bool_equals_string_column(indoc! {r"
    schema: pg_temp
    table: where_rows
    srid: 4326
    geometry_column: geom
    geometry_type: GEOMETRY
    id_column: id
    properties:
      b: bool
      i: int4
      f: float8
      s: text
      d: date
      n: int8
    layers:
      l:
        where: 'b == s'
"}, &[3])]
#[case::bool_not_equals_string_column(indoc! {r"
    schema: pg_temp
    table: where_rows
    srid: 4326
    geometry_column: geom
    geometry_type: GEOMETRY
    id_column: id
    properties:
      b: bool
      i: int4
      f: float8
      s: text
      d: date
      n: int8
    layers:
      l:
        where: 'b != s'
"}, &[1, 2, 4, 5, 6, 7, 8])]
#[case::and(indoc! {r"
    schema: pg_temp
    table: where_rows
    srid: 4326
    geometry_column: geom
    geometry_type: GEOMETRY
    id_column: id
    properties:
      b: bool
      i: int4
      f: float8
      s: text
      d: date
      n: int8
    layers:
      l:
        where: 'b && i > 0'
"}, &[1, 7])]
#[case::negated_and(indoc! {r"
    schema: pg_temp
    table: where_rows
    srid: 4326
    geometry_column: geom
    geometry_type: GEOMETRY
    id_column: id
    properties:
      b: bool
      i: int4
      f: float8
      s: text
      d: date
      n: int8
    layers:
      l:
        where: '!(b && i > 0)'
"}, &[2, 4, 5, 8])]
#[case::or(indoc! {r#"
    schema: pg_temp
    table: where_rows
    srid: 4326
    geometry_column: geom
    geometry_type: GEOMETRY
    id_column: id
    properties:
      b: bool
      i: int4
      f: float8
      s: text
      d: date
      n: int8
    layers:
      l:
        where: "b || s == ''"
"#}, &[1, 4, 6, 7])]
#[case::negated_or(indoc! {r"
    schema: pg_temp
    table: where_rows
    srid: 4326
    geometry_column: geom
    geometry_type: GEOMETRY
    id_column: id
    properties:
      b: bool
      i: int4
      f: float8
      s: text
      d: date
      n: int8
    layers:
      l:
        where: '!(b || f > 1.0)'
"}, &[])]
#[case::conditional(indoc! {r#"
    schema: pg_temp
    table: where_rows
    srid: 4326
    geometry_column: geom
    geometry_type: GEOMETRY
    id_column: id
    properties:
      b: bool
      i: int4
      f: float8
      s: text
      d: date
      n: int8
    layers:
      l:
        where: "b ? i > 0 : s == ''"
"#}, &[1, 7])]
#[case::negated_conditional(indoc! {r#"
    schema: pg_temp
    table: where_rows
    srid: 4326
    geometry_column: geom
    geometry_type: GEOMETRY
    id_column: id
    properties:
      b: bool
      i: int4
      f: float8
      s: text
      d: date
      n: int8
    layers:
      l:
        where: "!(b ? i > 0 : s == '')"
"#}, &[2, 4, 5, 8])]
#[case::false_literal(indoc! {r"
    schema: pg_temp
    table: where_rows
    srid: 4326
    geometry_column: geom
    geometry_type: GEOMETRY
    id_column: id
    properties:
      b: bool
      i: int4
      f: float8
      s: text
      d: date
      n: int8
    layers:
      l:
        where: 'false'
"}, &[])]
#[case::point_geometry(indoc! {r"
    schema: pg_temp
    table: where_rows
    srid: 4326
    geometry_column: geom
    geometry_type: GEOMETRY
    id_column: id
    properties:
      b: bool
      i: int4
      f: float8
      s: text
      d: date
      n: int8
    layers:
      l:
        geometry: point
"}, &[1, 4, 5, 8])]
#[case::line_geometry(indoc! {r"
    schema: pg_temp
    table: where_rows
    srid: 4326
    geometry_column: geom
    geometry_type: GEOMETRY
    id_column: id
    properties:
      b: bool
      i: int4
      f: float8
      s: text
      d: date
      n: int8
    layers:
      l:
        geometry: line
"}, &[2, 7])]
#[case::polygon_geometry_and_where(indoc! {r"
    schema: pg_temp
    table: where_rows
    srid: 4326
    geometry_column: geom
    geometry_type: GEOMETRY
    id_column: id
    properties:
      b: bool
      i: int4
      f: float8
      s: text
      d: date
      n: int8
    layers:
      l:
        geometry: polygon
        where: 'b != true'
"}, &[3, 6])]
#[tokio::test]
async fn sql_keeps_the_rows_the_layer_takes(#[case] yaml: &str, #[case] ids: &[u64]) {
    let client = connect().await;
    let (in_sql, in_plan) = kept(&client, yaml).await;
    assert_eq!(in_plan, ids.iter().copied().collect::<BTreeSet<_>>());
    assert_eq!(in_sql, in_plan);
}
