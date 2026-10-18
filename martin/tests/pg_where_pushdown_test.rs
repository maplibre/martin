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

/// A connection to the test database that has run `rows`.
async fn connect(rows: &str) -> PooledConnection {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL");
    let pool = PostgresPool::new(&url, None, None, None, 1, RetryTimeout::default())
        .await
        .unwrap();
    let client = pool.get().await.unwrap();
    client.batch_execute(rows).await.unwrap();
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

#[tokio::test]
async fn sql_matches_the_plan_on_bool_column() {
    let client = connect(indoc! {r"
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
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', true, 2, '-Infinity', 'a\b', NULL, NULL),
          (8, 'SRID=4326;POINT(2 2)', false, -1, 3.25, 'abd', NULL, NULL);
    "})
    .await;
    let (in_sql, in_plan) = kept(
        &client,
        indoc! {r"
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
        "},
    )
    .await;
    assert_eq!(in_sql, in_plan);
    insta::assert_compact_debug_snapshot!(in_sql, @"{1, 4, 7}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_negated_bool_column() {
    let client = connect(indoc! {r"
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
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', true, 2, '-Infinity', 'a\b', NULL, NULL),
          (8, 'SRID=4326;POINT(2 2)', false, -1, 3.25, 'abd', NULL, NULL);
    "})
    .await;
    let (in_sql, in_plan) = kept(
        &client,
        indoc! {r"
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
        "},
    )
    .await;
    assert_eq!(in_sql, in_plan);
    insta::assert_compact_debug_snapshot!(in_sql, @"{2, 5, 8}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_bool_equals_true() {
    let client = connect(indoc! {r"
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
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', true, 2, '-Infinity', 'a\b', NULL, NULL),
          (8, 'SRID=4326;POINT(2 2)', false, -1, 3.25, 'abd', NULL, NULL);
    "})
    .await;
    let (in_sql, in_plan) = kept(
        &client,
        indoc! {r"
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
        "},
    )
    .await;
    assert_eq!(in_sql, in_plan);
    insta::assert_compact_debug_snapshot!(in_sql, @"{1, 4, 7}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_bool_not_equals_true() {
    let client = connect(indoc! {r"
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
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', true, 2, '-Infinity', 'a\b', NULL, NULL),
          (8, 'SRID=4326;POINT(2 2)', false, -1, 3.25, 'abd', NULL, NULL);
    "})
    .await;
    let (in_sql, in_plan) = kept(
        &client,
        indoc! {r"
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
        "},
    )
    .await;
    assert_eq!(in_sql, in_plan);
    insta::assert_compact_debug_snapshot!(in_sql, @"{2, 3, 5, 6, 8}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_bool_equals_null() {
    let client = connect(indoc! {r"
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
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', true, 2, '-Infinity', 'a\b', NULL, NULL),
          (8, 'SRID=4326;POINT(2 2)', false, -1, 3.25, 'abd', NULL, NULL);
    "})
    .await;
    let (in_sql, in_plan) = kept(
        &client,
        indoc! {r"
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
        "},
    )
    .await;
    assert_eq!(in_sql, in_plan);
    insta::assert_compact_debug_snapshot!(in_sql, @"{3, 6}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_bool_not_equals_null() {
    let client = connect(indoc! {r"
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
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', true, 2, '-Infinity', 'a\b', NULL, NULL),
          (8, 'SRID=4326;POINT(2 2)', false, -1, 3.25, 'abd', NULL, NULL);
    "})
    .await;
    let (in_sql, in_plan) = kept(
        &client,
        indoc! {r"
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
        "},
    )
    .await;
    assert_eq!(in_sql, in_plan);
    insta::assert_compact_debug_snapshot!(in_sql, @"{1, 2, 4, 5, 7, 8}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_int_greater() {
    let client = connect(indoc! {r"
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
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', true, 2, '-Infinity', 'a\b', NULL, NULL),
          (8, 'SRID=4326;POINT(2 2)', false, -1, 3.25, 'abd', NULL, NULL);
    "})
    .await;
    let (in_sql, in_plan) = kept(
        &client,
        indoc! {r"
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
        "},
    )
    .await;
    assert_eq!(in_sql, in_plan);
    insta::assert_compact_debug_snapshot!(in_sql, @"{5, 6, 7}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_negated_int_greater() {
    let client = connect(indoc! {r"
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
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', true, 2, '-Infinity', 'a\b', NULL, NULL),
          (8, 'SRID=4326;POINT(2 2)', false, -1, 3.25, 'abd', NULL, NULL);
    "})
    .await;
    let (in_sql, in_plan) = kept(
        &client,
        indoc! {r"
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
        "},
    )
    .await;
    assert_eq!(in_sql, in_plan);
    insta::assert_compact_debug_snapshot!(in_sql, @"{1, 2, 4, 8}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_int_at_most_float() {
    let client = connect(indoc! {r"
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
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', true, 2, '-Infinity', 'a\b', NULL, NULL),
          (8, 'SRID=4326;POINT(2 2)', false, -1, 3.25, 'abd', NULL, NULL);
    "})
    .await;
    let (in_sql, in_plan) = kept(
        &client,
        indoc! {r"
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
        "},
    )
    .await;
    assert_eq!(in_sql, in_plan);
    insta::assert_compact_debug_snapshot!(in_sql, @"{1, 2, 4, 8}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_int_equals() {
    let client = connect(indoc! {r"
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
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', true, 2, '-Infinity', 'a\b', NULL, NULL),
          (8, 'SRID=4326;POINT(2 2)', false, -1, 3.25, 'abd', NULL, NULL);
    "})
    .await;
    let (in_sql, in_plan) = kept(
        &client,
        indoc! {r"
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
        "},
    )
    .await;
    assert_eq!(in_sql, in_plan);
    insta::assert_compact_debug_snapshot!(in_sql, @"{6}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_int_not_equals() {
    let client = connect(indoc! {r"
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
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', true, 2, '-Infinity', 'a\b', NULL, NULL),
          (8, 'SRID=4326;POINT(2 2)', false, -1, 3.25, 'abd', NULL, NULL);
    "})
    .await;
    let (in_sql, in_plan) = kept(
        &client,
        indoc! {r"
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
        "},
    )
    .await;
    assert_eq!(in_sql, in_plan);
    insta::assert_compact_debug_snapshot!(in_sql, @"{1, 2, 3, 4, 5, 7, 8}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_negative_literal_less_than_int() {
    let client = connect(indoc! {r"
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
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', true, 2, '-Infinity', 'a\b', NULL, NULL),
          (8, 'SRID=4326;POINT(2 2)', false, -1, 3.25, 'abd', NULL, NULL);
    "})
    .await;
    let (in_sql, in_plan) = kept(
        &client,
        indoc! {r"
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
        "},
    )
    .await;
    assert_eq!(in_sql, in_plan);
    insta::assert_compact_debug_snapshot!(in_sql, @"{1, 4, 5, 6, 7}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_int_in_list_with_null() {
    let client = connect(indoc! {r"
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
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', true, 2, '-Infinity', 'a\b', NULL, NULL),
          (8, 'SRID=4326;POINT(2 2)', false, -1, 3.25, 'abd', NULL, NULL);
    "})
    .await;
    let (in_sql, in_plan) = kept(
        &client,
        indoc! {r"
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
        "},
    )
    .await;
    assert_eq!(in_sql, in_plan);
    insta::assert_compact_debug_snapshot!(in_sql, @"{1, 3, 7}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_int_not_in_list() {
    let client = connect(indoc! {r"
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
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', true, 2, '-Infinity', 'a\b', NULL, NULL),
          (8, 'SRID=4326;POINT(2 2)', false, -1, 3.25, 'abd', NULL, NULL);
    "})
    .await;
    let (in_sql, in_plan) = kept(
        &client,
        indoc! {r"
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
        "},
    )
    .await;
    assert_eq!(in_sql, in_plan);
    insta::assert_compact_debug_snapshot!(in_sql, @"{2, 3, 4, 5, 6, 8}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_int_not_in_list_with_null() {
    let client = connect(indoc! {r"
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
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', true, 2, '-Infinity', 'a\b', NULL, NULL),
          (8, 'SRID=4326;POINT(2 2)', false, -1, 3.25, 'abd', NULL, NULL);
    "})
    .await;
    let (in_sql, in_plan) = kept(
        &client,
        indoc! {r"
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
        "},
    )
    .await;
    assert_eq!(in_sql, in_plan);
    insta::assert_compact_debug_snapshot!(in_sql, @"{2, 4, 5, 6, 7, 8}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_int_in_empty_list() {
    let client = connect(indoc! {r"
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
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', true, 2, '-Infinity', 'a\b', NULL, NULL),
          (8, 'SRID=4326;POINT(2 2)', false, -1, 3.25, 'abd', NULL, NULL);
    "})
    .await;
    let (in_sql, in_plan) = kept(
        &client,
        indoc! {r"
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
        "},
    )
    .await;
    assert_eq!(in_sql, in_plan);
    insta::assert_compact_debug_snapshot!(in_sql, @"{}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_int_not_in_empty_list() {
    let client = connect(indoc! {r"
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
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', true, 2, '-Infinity', 'a\b', NULL, NULL),
          (8, 'SRID=4326;POINT(2 2)', false, -1, 3.25, 'abd', NULL, NULL);
    "})
    .await;
    let (in_sql, in_plan) = kept(
        &client,
        indoc! {r"
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
        "},
    )
    .await;
    assert_eq!(in_sql, in_plan);
    insta::assert_compact_debug_snapshot!(in_sql, @"{1, 2, 3, 4, 5, 6, 7, 8}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_float_greater() {
    let client = connect(indoc! {r"
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
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', true, 2, '-Infinity', 'a\b', NULL, NULL),
          (8, 'SRID=4326;POINT(2 2)', false, -1, 3.25, 'abd', NULL, NULL);
    "})
    .await;
    let (in_sql, in_plan) = kept(
        &client,
        indoc! {r"
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
        "},
    )
    .await;
    assert_eq!(in_sql, in_plan);
    insta::assert_compact_debug_snapshot!(in_sql, @"{1, 5, 6, 8}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_negated_float_greater() {
    let client = connect(indoc! {r"
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
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', true, 2, '-Infinity', 'a\b', NULL, NULL),
          (8, 'SRID=4326;POINT(2 2)', false, -1, 3.25, 'abd', NULL, NULL);
    "})
    .await;
    let (in_sql, in_plan) = kept(
        &client,
        indoc! {r"
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
        "},
    )
    .await;
    assert_eq!(in_sql, in_plan);
    insta::assert_compact_debug_snapshot!(in_sql, @"{4, 7}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_float_equals_int() {
    let client = connect(indoc! {r"
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
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', true, 2, '-Infinity', 'a\b', NULL, NULL),
          (8, 'SRID=4326;POINT(2 2)', false, -1, 3.25, 'abd', NULL, NULL);
    "})
    .await;
    let (in_sql, in_plan) = kept(
        &client,
        indoc! {r"
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
        "},
    )
    .await;
    assert_eq!(in_sql, in_plan);
    insta::assert_compact_debug_snapshot!(in_sql, @"{6}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_float_not_equals_int() {
    let client = connect(indoc! {r"
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
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', true, 2, '-Infinity', 'a\b', NULL, NULL),
          (8, 'SRID=4326;POINT(2 2)', false, -1, 3.25, 'abd', NULL, NULL);
    "})
    .await;
    let (in_sql, in_plan) = kept(
        &client,
        indoc! {r"
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
        "},
    )
    .await;
    assert_eq!(in_sql, in_plan);
    insta::assert_compact_debug_snapshot!(in_sql, @"{1, 2, 3, 4, 5, 7, 8}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_float_less_than_int() {
    let client = connect(indoc! {r"
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
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', true, 2, '-Infinity', 'a\b', NULL, NULL),
          (8, 'SRID=4326;POINT(2 2)', false, -1, 3.25, 'abd', NULL, NULL);
    "})
    .await;
    let (in_sql, in_plan) = kept(
        &client,
        indoc! {r"
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
        "},
    )
    .await;
    assert_eq!(in_sql, in_plan);
    insta::assert_compact_debug_snapshot!(in_sql, @"{7}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_negated_float_less_than_int() {
    let client = connect(indoc! {r"
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
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', true, 2, '-Infinity', 'a\b', NULL, NULL),
          (8, 'SRID=4326;POINT(2 2)', false, -1, 3.25, 'abd', NULL, NULL);
    "})
    .await;
    let (in_sql, in_plan) = kept(
        &client,
        indoc! {r"
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
        "},
    )
    .await;
    assert_eq!(in_sql, in_plan);
    insta::assert_compact_debug_snapshot!(in_sql, @"{1, 4, 5, 6, 8}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_float_in_list() {
    let client = connect(indoc! {r"
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
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', true, 2, '-Infinity', 'a\b', NULL, NULL),
          (8, 'SRID=4326;POINT(2 2)', false, -1, 3.25, 'abd', NULL, NULL);
    "})
    .await;
    let (in_sql, in_plan) = kept(
        &client,
        indoc! {r"
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
        "},
    )
    .await;
    assert_eq!(in_sql, in_plan);
    insta::assert_compact_debug_snapshot!(in_sql, @"{1, 6}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_float_not_in_list() {
    let client = connect(indoc! {r"
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
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', true, 2, '-Infinity', 'a\b', NULL, NULL),
          (8, 'SRID=4326;POINT(2 2)', false, -1, 3.25, 'abd', NULL, NULL);
    "})
    .await;
    let (in_sql, in_plan) = kept(
        &client,
        indoc! {r"
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
        "},
    )
    .await;
    assert_eq!(in_sql, in_plan);
    insta::assert_compact_debug_snapshot!(in_sql, @"{2, 3, 4, 5, 7, 8}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_int_equals_float_column() {
    let client = connect(indoc! {r"
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
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', true, 2, '-Infinity', 'a\b', NULL, NULL),
          (8, 'SRID=4326;POINT(2 2)', false, -1, 3.25, 'abd', NULL, NULL);
    "})
    .await;
    let (in_sql, in_plan) = kept(
        &client,
        indoc! {r"
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
        "},
    )
    .await;
    assert_eq!(in_sql, in_plan);
    insta::assert_compact_debug_snapshot!(in_sql, @"{3, 4}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_int_not_equals_float_column() {
    let client = connect(indoc! {r"
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
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', true, 2, '-Infinity', 'a\b', NULL, NULL),
          (8, 'SRID=4326;POINT(2 2)', false, -1, 3.25, 'abd', NULL, NULL);
    "})
    .await;
    let (in_sql, in_plan) = kept(
        &client,
        indoc! {r"
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
        "},
    )
    .await;
    assert_eq!(in_sql, in_plan);
    insta::assert_compact_debug_snapshot!(in_sql, @"{1, 2, 5, 6, 7, 8}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_int8_in_list_of_float_and_int() {
    let client = connect(indoc! {r"
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
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', true, 2, '-Infinity', 'a\b', NULL, NULL),
          (8, 'SRID=4326;POINT(2 2)', false, -1, 3.25, 'abd', NULL, NULL);
    "})
    .await;
    let (in_sql, in_plan) = kept(
        &client,
        indoc! {r"
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
        "},
    )
    .await;
    assert_eq!(in_sql, in_plan);
    insta::assert_compact_debug_snapshot!(in_sql, @"{}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_int8_in_list_of_int() {
    let client = connect(indoc! {r"
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
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', true, 2, '-Infinity', 'a\b', NULL, NULL),
          (8, 'SRID=4326;POINT(2 2)', false, -1, 3.25, 'abd', NULL, NULL);
    "})
    .await;
    let (in_sql, in_plan) = kept(
        &client,
        indoc! {r"
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
        "},
    )
    .await;
    assert_eq!(in_sql, in_plan);
    insta::assert_compact_debug_snapshot!(in_sql, @"{1}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_string_equals() {
    let client = connect(indoc! {r"
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
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', true, 2, '-Infinity', 'a\b', NULL, NULL),
          (8, 'SRID=4326;POINT(2 2)', false, -1, 3.25, 'abd', NULL, NULL);
    "})
    .await;
    let (in_sql, in_plan) = kept(
        &client,
        indoc! {r#"
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
        "#},
    )
    .await;
    assert_eq!(in_sql, in_plan);
    insta::assert_compact_debug_snapshot!(in_sql, @"{1}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_string_not_equals() {
    let client = connect(indoc! {r"
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
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', true, 2, '-Infinity', 'a\b', NULL, NULL),
          (8, 'SRID=4326;POINT(2 2)', false, -1, 3.25, 'abd', NULL, NULL);
    "})
    .await;
    let (in_sql, in_plan) = kept(
        &client,
        indoc! {r#"
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
        "#},
    )
    .await;
    assert_eq!(in_sql, in_plan);
    insta::assert_compact_debug_snapshot!(in_sql, @"{2, 3, 4, 5, 6, 7, 8}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_string_less() {
    let client = connect(indoc! {r"
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
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', true, 2, '-Infinity', 'a\b', NULL, NULL),
          (8, 'SRID=4326;POINT(2 2)', false, -1, 3.25, 'abd', NULL, NULL);
    "})
    .await;
    let (in_sql, in_plan) = kept(
        &client,
        indoc! {r#"
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
        "#},
    )
    .await;
    assert_eq!(in_sql, in_plan);
    insta::assert_compact_debug_snapshot!(in_sql, @"{1, 2, 4, 6, 7}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_negated_string_less() {
    let client = connect(indoc! {r"
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
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', true, 2, '-Infinity', 'a\b', NULL, NULL),
          (8, 'SRID=4326;POINT(2 2)', false, -1, 3.25, 'abd', NULL, NULL);
    "})
    .await;
    let (in_sql, in_plan) = kept(
        &client,
        indoc! {r#"
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
        "#},
    )
    .await;
    assert_eq!(in_sql, in_plan);
    insta::assert_compact_debug_snapshot!(in_sql, @"{5, 8}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_string_at_least() {
    let client = connect(indoc! {r"
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
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', true, 2, '-Infinity', 'a\b', NULL, NULL),
          (8, 'SRID=4326;POINT(2 2)', false, -1, 3.25, 'abd', NULL, NULL);
    "})
    .await;
    let (in_sql, in_plan) = kept(
        &client,
        indoc! {r#"
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
        "#},
    )
    .await;
    assert_eq!(in_sql, in_plan);
    insta::assert_compact_debug_snapshot!(in_sql, @"{1, 2, 5, 7, 8}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_starts_with_percent() {
    let client = connect(indoc! {r"
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
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', true, 2, '-Infinity', 'a\b', NULL, NULL),
          (8, 'SRID=4326;POINT(2 2)', false, -1, 3.25, 'abd', NULL, NULL);
    "})
    .await;
    let (in_sql, in_plan) = kept(
        &client,
        indoc! {r#"
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
        "#},
    )
    .await;
    assert_eq!(in_sql, in_plan);
    insta::assert_compact_debug_snapshot!(in_sql, @"{2}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_ends_with() {
    let client = connect(indoc! {r"
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
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', true, 2, '-Infinity', 'a\b', NULL, NULL),
          (8, 'SRID=4326;POINT(2 2)', false, -1, 3.25, 'abd', NULL, NULL);
    "})
    .await;
    let (in_sql, in_plan) = kept(
        &client,
        indoc! {r#"
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
        "#},
    )
    .await;
    assert_eq!(in_sql, in_plan);
    insta::assert_compact_debug_snapshot!(in_sql, @"{1, 2}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_contains_underscore() {
    let client = connect(indoc! {r"
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
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', true, 2, '-Infinity', 'a\b', NULL, NULL),
          (8, 'SRID=4326;POINT(2 2)', false, -1, 3.25, 'abd', NULL, NULL);
    "})
    .await;
    let (in_sql, in_plan) = kept(
        &client,
        indoc! {r#"
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
        "#},
    )
    .await;
    assert_eq!(in_sql, in_plan);
    insta::assert_compact_debug_snapshot!(in_sql, @"{2}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_contains_backslash() {
    let client = connect(indoc! {r"
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
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', true, 2, '-Infinity', 'a\b', NULL, NULL),
          (8, 'SRID=4326;POINT(2 2)', false, -1, 3.25, 'abd', NULL, NULL);
    "})
    .await;
    let (in_sql, in_plan) = kept(
        &client,
        indoc! {r#"
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
        "#},
    )
    .await;
    assert_eq!(in_sql, in_plan);
    insta::assert_compact_debug_snapshot!(in_sql, @"{7}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_negated_starts_with() {
    let client = connect(indoc! {r"
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
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', true, 2, '-Infinity', 'a\b', NULL, NULL),
          (8, 'SRID=4326;POINT(2 2)', false, -1, 3.25, 'abd', NULL, NULL);
    "})
    .await;
    let (in_sql, in_plan) = kept(
        &client,
        indoc! {r#"
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
        "#},
    )
    .await;
    assert_eq!(in_sql, in_plan);
    insta::assert_compact_debug_snapshot!(in_sql, @"{4, 5, 6}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_size_equals() {
    let client = connect(indoc! {r"
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
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', true, 2, '-Infinity', 'a\b', NULL, NULL),
          (8, 'SRID=4326;POINT(2 2)', false, -1, 3.25, 'abd', NULL, NULL);
    "})
    .await;
    let (in_sql, in_plan) = kept(
        &client,
        indoc! {r"
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
        "},
    )
    .await;
    assert_eq!(in_sql, in_plan);
    insta::assert_compact_debug_snapshot!(in_sql, @"{1, 4, 7, 8}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_size_not_equals() {
    let client = connect(indoc! {r"
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
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', true, 2, '-Infinity', 'a\b', NULL, NULL),
          (8, 'SRID=4326;POINT(2 2)', false, -1, 3.25, 'abd', NULL, NULL);
    "})
    .await;
    let (in_sql, in_plan) = kept(
        &client,
        indoc! {r"
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
        "},
    )
    .await;
    assert_eq!(in_sql, in_plan);
    insta::assert_compact_debug_snapshot!(in_sql, @"{2, 5, 6}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_size_greater() {
    let client = connect(indoc! {r"
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
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', true, 2, '-Infinity', 'a\b', NULL, NULL),
          (8, 'SRID=4326;POINT(2 2)', false, -1, 3.25, 'abd', NULL, NULL);
    "})
    .await;
    let (in_sql, in_plan) = kept(
        &client,
        indoc! {r"
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
        "},
    )
    .await;
    assert_eq!(in_sql, in_plan);
    insta::assert_compact_debug_snapshot!(in_sql, @"{1, 2, 4, 5, 7, 8}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_lower_ascii_equals() {
    let client = connect(indoc! {r"
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
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', true, 2, '-Infinity', 'a\b', NULL, NULL),
          (8, 'SRID=4326;POINT(2 2)', false, -1, 3.25, 'abd', NULL, NULL);
    "})
    .await;
    let (in_sql, in_plan) = kept(
        &client,
        indoc! {r#"
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
        "#},
    )
    .await;
    assert_eq!(in_sql, in_plan);
    insta::assert_compact_debug_snapshot!(in_sql, @"{1, 4}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_upper_ascii_equals() {
    let client = connect(indoc! {r"
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
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', true, 2, '-Infinity', 'a\b', NULL, NULL),
          (8, 'SRID=4326;POINT(2 2)', false, -1, 3.25, 'abd', NULL, NULL);
    "})
    .await;
    let (in_sql, in_plan) = kept(
        &client,
        indoc! {r#"
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
        "#},
    )
    .await;
    assert_eq!(in_sql, in_plan);
    insta::assert_compact_debug_snapshot!(in_sql, @"{5}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_string_in_list() {
    let client = connect(indoc! {r"
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
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', true, 2, '-Infinity', 'a\b', NULL, NULL),
          (8, 'SRID=4326;POINT(2 2)', false, -1, 3.25, 'abd', NULL, NULL);
    "})
    .await;
    let (in_sql, in_plan) = kept(
        &client,
        indoc! {r#"
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
        "#},
    )
    .await;
    assert_eq!(in_sql, in_plan);
    insta::assert_compact_debug_snapshot!(in_sql, @"{1, 4}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_string_not_in_list() {
    let client = connect(indoc! {r"
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
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', true, 2, '-Infinity', 'a\b', NULL, NULL),
          (8, 'SRID=4326;POINT(2 2)', false, -1, 3.25, 'abd', NULL, NULL);
    "})
    .await;
    let (in_sql, in_plan) = kept(
        &client,
        indoc! {r#"
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
        "#},
    )
    .await;
    assert_eq!(in_sql, in_plan);
    insta::assert_compact_debug_snapshot!(in_sql, @"{2, 3, 5, 6, 7, 8}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_has_field() {
    let client = connect(indoc! {r"
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
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', true, 2, '-Infinity', 'a\b', NULL, NULL),
          (8, 'SRID=4326;POINT(2 2)', false, -1, 3.25, 'abd', NULL, NULL);
    "})
    .await;
    let (in_sql, in_plan) = kept(
        &client,
        indoc! {r"
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
        "},
    )
    .await;
    assert_eq!(in_sql, in_plan);
    insta::assert_compact_debug_snapshot!(in_sql, @"{1, 2, 4, 5, 6, 7, 8}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_negated_has_field() {
    let client = connect(indoc! {r"
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
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', true, 2, '-Infinity', 'a\b', NULL, NULL),
          (8, 'SRID=4326;POINT(2 2)', false, -1, 3.25, 'abd', NULL, NULL);
    "})
    .await;
    let (in_sql, in_plan) = kept(
        &client,
        indoc! {r"
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
        "},
    )
    .await;
    assert_eq!(in_sql, in_plan);
    insta::assert_compact_debug_snapshot!(in_sql, @"{3}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_key_in_feature() {
    let client = connect(indoc! {r"
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
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', true, 2, '-Infinity', 'a\b', NULL, NULL),
          (8, 'SRID=4326;POINT(2 2)', false, -1, 3.25, 'abd', NULL, NULL);
    "})
    .await;
    let (in_sql, in_plan) = kept(
        &client,
        indoc! {r#"
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
        "#},
    )
    .await;
    assert_eq!(in_sql, in_plan);
    insta::assert_compact_debug_snapshot!(in_sql, @"{1, 4, 6}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_feature_index_equals() {
    let client = connect(indoc! {r"
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
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', true, 2, '-Infinity', 'a\b', NULL, NULL),
          (8, 'SRID=4326;POINT(2 2)', false, -1, 3.25, 'abd', NULL, NULL);
    "})
    .await;
    let (in_sql, in_plan) = kept(
        &client,
        indoc! {r#"
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
        "#},
    )
    .await;
    assert_eq!(in_sql, in_plan);
    insta::assert_compact_debug_snapshot!(in_sql, @"{1}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_feature_field_equals() {
    let client = connect(indoc! {r"
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
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', true, 2, '-Infinity', 'a\b', NULL, NULL),
          (8, 'SRID=4326;POINT(2 2)', false, -1, 3.25, 'abd', NULL, NULL);
    "})
    .await;
    let (in_sql, in_plan) = kept(
        &client,
        indoc! {r#"
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
        "#},
    )
    .await;
    assert_eq!(in_sql, in_plan);
    insta::assert_compact_debug_snapshot!(in_sql, @"{1}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_date_equals() {
    let client = connect(indoc! {r"
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
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', true, 2, '-Infinity', 'a\b', NULL, NULL),
          (8, 'SRID=4326;POINT(2 2)', false, -1, 3.25, 'abd', NULL, NULL);
    "})
    .await;
    let (in_sql, in_plan) = kept(
        &client,
        indoc! {r#"
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
        "#},
    )
    .await;
    assert_eq!(in_sql, in_plan);
    insta::assert_compact_debug_snapshot!(in_sql, @"{1, 6}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_date_greater() {
    let client = connect(indoc! {r"
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
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', true, 2, '-Infinity', 'a\b', NULL, NULL),
          (8, 'SRID=4326;POINT(2 2)', false, -1, 3.25, 'abd', NULL, NULL);
    "})
    .await;
    let (in_sql, in_plan) = kept(
        &client,
        indoc! {r#"
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
        "#},
    )
    .await;
    assert_eq!(in_sql, in_plan);
    insta::assert_compact_debug_snapshot!(in_sql, @"{1, 6}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_string_equals_int() {
    let client = connect(indoc! {r"
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
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', true, 2, '-Infinity', 'a\b', NULL, NULL),
          (8, 'SRID=4326;POINT(2 2)', false, -1, 3.25, 'abd', NULL, NULL);
    "})
    .await;
    let (in_sql, in_plan) = kept(
        &client,
        indoc! {r"
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
        "},
    )
    .await;
    assert_eq!(in_sql, in_plan);
    insta::assert_compact_debug_snapshot!(in_sql, @"{}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_string_not_equals_int() {
    let client = connect(indoc! {r"
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
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', true, 2, '-Infinity', 'a\b', NULL, NULL),
          (8, 'SRID=4326;POINT(2 2)', false, -1, 3.25, 'abd', NULL, NULL);
    "})
    .await;
    let (in_sql, in_plan) = kept(
        &client,
        indoc! {r"
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
        "},
    )
    .await;
    assert_eq!(in_sql, in_plan);
    insta::assert_compact_debug_snapshot!(in_sql, @"{1, 2, 3, 4, 5, 6, 7, 8}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_int_equals_string() {
    let client = connect(indoc! {r"
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
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', true, 2, '-Infinity', 'a\b', NULL, NULL),
          (8, 'SRID=4326;POINT(2 2)', false, -1, 3.25, 'abd', NULL, NULL);
    "})
    .await;
    let (in_sql, in_plan) = kept(
        &client,
        indoc! {r#"
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
        "#},
    )
    .await;
    assert_eq!(in_sql, in_plan);
    insta::assert_compact_debug_snapshot!(in_sql, @"{}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_string_greater_than_int() {
    let client = connect(indoc! {r"
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
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', true, 2, '-Infinity', 'a\b', NULL, NULL),
          (8, 'SRID=4326;POINT(2 2)', false, -1, 3.25, 'abd', NULL, NULL);
    "})
    .await;
    let (in_sql, in_plan) = kept(
        &client,
        indoc! {r"
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
        "},
    )
    .await;
    assert_eq!(in_sql, in_plan);
    insta::assert_compact_debug_snapshot!(in_sql, @"{}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_negated_string_greater_than_int() {
    let client = connect(indoc! {r"
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
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', true, 2, '-Infinity', 'a\b', NULL, NULL),
          (8, 'SRID=4326;POINT(2 2)', false, -1, 3.25, 'abd', NULL, NULL);
    "})
    .await;
    let (in_sql, in_plan) = kept(
        &client,
        indoc! {r"
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
        "},
    )
    .await;
    assert_eq!(in_sql, in_plan);
    insta::assert_compact_debug_snapshot!(in_sql, @"{}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_bool_equals_string_column() {
    let client = connect(indoc! {r"
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
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', true, 2, '-Infinity', 'a\b', NULL, NULL),
          (8, 'SRID=4326;POINT(2 2)', false, -1, 3.25, 'abd', NULL, NULL);
    "})
    .await;
    let (in_sql, in_plan) = kept(
        &client,
        indoc! {r"
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
        "},
    )
    .await;
    assert_eq!(in_sql, in_plan);
    insta::assert_compact_debug_snapshot!(in_sql, @"{3}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_bool_not_equals_string_column() {
    let client = connect(indoc! {r"
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
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', true, 2, '-Infinity', 'a\b', NULL, NULL),
          (8, 'SRID=4326;POINT(2 2)', false, -1, 3.25, 'abd', NULL, NULL);
    "})
    .await;
    let (in_sql, in_plan) = kept(
        &client,
        indoc! {r"
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
        "},
    )
    .await;
    assert_eq!(in_sql, in_plan);
    insta::assert_compact_debug_snapshot!(in_sql, @"{1, 2, 4, 5, 6, 7, 8}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_and() {
    let client = connect(indoc! {r"
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
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', true, 2, '-Infinity', 'a\b', NULL, NULL),
          (8, 'SRID=4326;POINT(2 2)', false, -1, 3.25, 'abd', NULL, NULL);
    "})
    .await;
    let (in_sql, in_plan) = kept(
        &client,
        indoc! {r"
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
        "},
    )
    .await;
    assert_eq!(in_sql, in_plan);
    insta::assert_compact_debug_snapshot!(in_sql, @"{1, 7}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_negated_and() {
    let client = connect(indoc! {r"
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
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', true, 2, '-Infinity', 'a\b', NULL, NULL),
          (8, 'SRID=4326;POINT(2 2)', false, -1, 3.25, 'abd', NULL, NULL);
    "})
    .await;
    let (in_sql, in_plan) = kept(
        &client,
        indoc! {r"
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
        "},
    )
    .await;
    assert_eq!(in_sql, in_plan);
    insta::assert_compact_debug_snapshot!(in_sql, @"{2, 4, 5, 8}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_or() {
    let client = connect(indoc! {r"
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
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', true, 2, '-Infinity', 'a\b', NULL, NULL),
          (8, 'SRID=4326;POINT(2 2)', false, -1, 3.25, 'abd', NULL, NULL);
    "})
    .await;
    let (in_sql, in_plan) = kept(
        &client,
        indoc! {r#"
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
        "#},
    )
    .await;
    assert_eq!(in_sql, in_plan);
    insta::assert_compact_debug_snapshot!(in_sql, @"{1, 4, 6, 7}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_negated_or() {
    let client = connect(indoc! {r"
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
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', true, 2, '-Infinity', 'a\b', NULL, NULL),
          (8, 'SRID=4326;POINT(2 2)', false, -1, 3.25, 'abd', NULL, NULL);
    "})
    .await;
    let (in_sql, in_plan) = kept(
        &client,
        indoc! {r"
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
        "},
    )
    .await;
    assert_eq!(in_sql, in_plan);
    insta::assert_compact_debug_snapshot!(in_sql, @"{}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_conditional() {
    let client = connect(indoc! {r"
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
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', true, 2, '-Infinity', 'a\b', NULL, NULL),
          (8, 'SRID=4326;POINT(2 2)', false, -1, 3.25, 'abd', NULL, NULL);
    "})
    .await;
    let (in_sql, in_plan) = kept(
        &client,
        indoc! {r#"
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
        "#},
    )
    .await;
    assert_eq!(in_sql, in_plan);
    insta::assert_compact_debug_snapshot!(in_sql, @"{1, 7}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_negated_conditional() {
    let client = connect(indoc! {r"
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
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', true, 2, '-Infinity', 'a\b', NULL, NULL),
          (8, 'SRID=4326;POINT(2 2)', false, -1, 3.25, 'abd', NULL, NULL);
    "})
    .await;
    let (in_sql, in_plan) = kept(
        &client,
        indoc! {r#"
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
        "#},
    )
    .await;
    assert_eq!(in_sql, in_plan);
    insta::assert_compact_debug_snapshot!(in_sql, @"{2, 4, 5, 8}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_false_literal() {
    let client = connect(indoc! {r"
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
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', true, 2, '-Infinity', 'a\b', NULL, NULL),
          (8, 'SRID=4326;POINT(2 2)', false, -1, 3.25, 'abd', NULL, NULL);
    "})
    .await;
    let (in_sql, in_plan) = kept(
        &client,
        indoc! {r"
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
        "},
    )
    .await;
    assert_eq!(in_sql, in_plan);
    insta::assert_compact_debug_snapshot!(in_sql, @"{}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_point_geometry() {
    let client = connect(indoc! {r"
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
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', true, 2, '-Infinity', 'a\b', NULL, NULL),
          (8, 'SRID=4326;POINT(2 2)', false, -1, 3.25, 'abd', NULL, NULL);
    "})
    .await;
    let (in_sql, in_plan) = kept(
        &client,
        indoc! {r"
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
        "},
    )
    .await;
    assert_eq!(in_sql, in_plan);
    insta::assert_compact_debug_snapshot!(in_sql, @"{1, 4, 5, 8}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_line_geometry() {
    let client = connect(indoc! {r"
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
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', true, 2, '-Infinity', 'a\b', NULL, NULL),
          (8, 'SRID=4326;POINT(2 2)', false, -1, 3.25, 'abd', NULL, NULL);
    "})
    .await;
    let (in_sql, in_plan) = kept(
        &client,
        indoc! {r"
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
        "},
    )
    .await;
    assert_eq!(in_sql, in_plan);
    insta::assert_compact_debug_snapshot!(in_sql, @"{2, 7}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_polygon_geometry_and_where() {
    let client = connect(indoc! {r"
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
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', true, 2, '-Infinity', 'a\b', NULL, NULL),
          (8, 'SRID=4326;POINT(2 2)', false, -1, 3.25, 'abd', NULL, NULL);
    "})
    .await;
    let (in_sql, in_plan) = kept(
        &client,
        indoc! {r"
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
        "},
    )
    .await;
    assert_eq!(in_sql, in_plan);
    insta::assert_compact_debug_snapshot!(in_sql, @"{3, 6}");
}
