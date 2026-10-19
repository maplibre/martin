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
        if !feature.geometry.decode_wkb() {
            continue;
        }
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
async fn sql_matches_the_plan_on_polygon_geometry() {
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
        "},
    )
    .await;
    assert_eq!(in_sql, in_plan);
    insta::assert_compact_debug_snapshot!(in_sql, @"{3, 6}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_line_geometry_of_curves_and_empty_lines() {
    let client = connect(indoc! {r"
        CREATE TEMP TABLE where_rows (id int8, geom geometry);
        INSERT INTO where_rows VALUES
          (1, 'SRID=4326;POINT EMPTY'),
          (2, 'SRID=4326;LINESTRING EMPTY'),
          (3, 'SRID=4326;POLYGON EMPTY'),
          (4, 'SRID=4326;CIRCULARSTRING(0 0 0, 1 1 0, 2 0 0)'),
          (5, 'SRID=4326;COMPOUNDCURVE((0 0 0, 1 1 0), CIRCULARSTRING(1 1 0, 2 0 0, 3 1 0))'),
          (6, 'SRID=4326;MULTICURVE((0 0 0, 1 1 0))'),
          (7, 'SRID=4326;CURVEPOLYGON(CIRCULARSTRING(0 0 0, 1 1 0, 2 0 0, 1 -1 0, 0 0 0))'),
          (8, 'SRID=4326;MULTISURFACE(((0 0 0, 1 0 0, 1 1 0, 0 0 0)))'),
          (9, 'SRID=4326;TRIANGLE((0 0 0, 1 0 0, 1 1 0, 0 0 0))'),
          (10, 'SRID=4326;TIN(((0 0 0, 1 0 0, 1 1 0, 0 0 0)))'),
          (11, 'SRID=4326;POLYHEDRALSURFACE(((0 0 0, 1 0 0, 1 1 0, 0 0 0)))'),
          (12, 'SRID=4326;GEOMETRYCOLLECTION(POINT(0 0 0), LINESTRING(0 0 0, 1 1 0))');
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
        layers:
          l:
            geometry: line
        "},
    )
    .await;
    assert_eq!(in_sql, in_plan);
    insta::assert_compact_debug_snapshot!(in_sql, @"{2, 4, 5, 6}");
}
