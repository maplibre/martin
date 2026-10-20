#![cfg(all(feature = "test-pg", feature = "unstable-generate"))]
#![expect(clippy::panic, clippy::unwrap_used)]

use std::collections::BTreeSet;

use indoc::indoc;
use martin::config::file::postgres::TableInfo;
use martin::config::file::postgres::resolver::{query_available_tables, scan_sql};
use martin::generate::layers::{LowerOptions, lower_table};
use martin_core::tiles::postgres::{PostgresPool, RetryTimeout, ScanLayout, scan_features};
use martin_tilegen::FeatureGeom;
use martin_tilegen::plan::{Plan, RowFilter};
use martin_tilegen::source::Geometry;

/// A pool of one connection to the test database, which has run `rows`.
async fn connect(rows: &str) -> PostgresPool {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL");
    let pool = PostgresPool::new(&url, None, None, None, 1, RetryTimeout::default())
        .await
        .unwrap();
    pool.get().await.unwrap().batch_execute(rows).await.unwrap();
    pool
}

/// The ids of the rows a table's layer keeps.
struct Kept {
    /// The rows the SQL condition keeps.
    by_sql: BTreeSet<u64>,
    /// The rows the plan's layer takes from an unfiltered scan.
    by_plan: BTreeSet<u64>,
}

/// The rows the SQL condition and the plan keep of the table `yaml` configures.
async fn kept(pool: &PostgresPool, yaml: &str) -> Kept {
    let options = LowerOptions {
        zooms: 0..=0,
        bounds: None,
    };
    let temp_schema: String = pool
        .get()
        .await
        .unwrap()
        .query_one("SELECT pg_my_temp_schema()::regnamespace::text", &[])
        .await
        .unwrap()
        .get(0);
    let discovered = query_available_tables(pool, Some(vec![temp_schema.clone()]), None)
        .await
        .unwrap();
    let configured: TableInfo = serde_saphyr::from_str(yaml).unwrap();
    let id = "where_rows".to_owned();
    let info = discovered[&temp_schema][&id][&configured.geometry_column]
        .append_cfg_info(&configured, &id, None)
        .unwrap();
    let scan = lower_table(&id, info, &options).unwrap().unwrap();
    let info = scan.info;
    let plan = Plan::new(vec![scan.def]).unwrap();
    let rows = plan.row_filter(0, 0..=0);
    if let RowFilter::Untranslatable(err) = &rows {
        panic!("{err}");
    }
    let filtered = scan_sql(&info, plan.columns(0), rows.condition(), None).unwrap();
    let client = pool.get().await.unwrap();
    let by_sql = client
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
    scan_features(&client, &all.sql, &[], &layout, 0, &plan.table_keys()[0], |batch| {
        features.extend(batch.features);
        Ok(())
    })
    .await
    .unwrap();
    let mut evaluator = plan.evaluator();
    let mut by_plan = BTreeSet::new();
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
                by_plan.insert(feature.id.unwrap());
                Ok(())
            })
            .unwrap();
    }
    Kept { by_sql, by_plan }
}

#[tokio::test]
async fn sql_matches_the_plan_on_point_geometry() {
    let pool = connect(indoc! {r"
        CREATE TEMP TABLE where_rows (id int8, geom geometry);
        INSERT INTO where_rows VALUES
          (1, 'SRID=4326;POINT(0 0)'),
          (2, 'SRID=4326;MULTIPOINT(0 0, 1 1)'),
          (3, 'SRID=4326;LINESTRING(0 0, 1 1)'),
          (4, 'SRID=4326;MULTILINESTRING((0 0, 1 1))'),
          (5, 'SRID=4326;POLYGON((0 0, 1 0, 1 1, 0 0))'),
          (6, 'SRID=4326;MULTIPOLYGON(((0 0, 1 0, 1 1, 0 0)))'),
          (7, 'SRID=4326;CIRCULARSTRING(0 0, 1 1, 2 0)'),
          (8, 'SRID=4326;CURVEPOLYGON(CIRCULARSTRING(0 0, 1 1, 2 0, 1 -1, 0 0))'),
          (9, 'SRID=4326;LINESTRING EMPTY'),
          (10, 'SRID=4326;GEOMETRYCOLLECTION(POINT(0 0), LINESTRING(0 0, 1 1))');
    "})
    .await;
    let kept = kept(
        &pool,
        indoc! {r"
        schema: pg_temp
        table: where_rows
        srid: 4326
        geometry_column: geom
        geometry_type: GEOMETRY
        id_column: id
        layers:
          l:
            geometry: point
        "},
    )
    .await;
    assert_eq!(kept.by_sql, kept.by_plan);
    insta::assert_compact_debug_snapshot!(kept.by_sql, @"{1, 2}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_line_geometry() {
    let pool = connect(indoc! {r"
        CREATE TEMP TABLE where_rows (id int8, geom geometry);
        INSERT INTO where_rows VALUES
          (1, 'SRID=4326;POINT(0 0)'),
          (2, 'SRID=4326;MULTIPOINT(0 0, 1 1)'),
          (3, 'SRID=4326;LINESTRING(0 0, 1 1)'),
          (4, 'SRID=4326;MULTILINESTRING((0 0, 1 1))'),
          (5, 'SRID=4326;POLYGON((0 0, 1 0, 1 1, 0 0))'),
          (6, 'SRID=4326;MULTIPOLYGON(((0 0, 1 0, 1 1, 0 0)))'),
          (7, 'SRID=4326;CIRCULARSTRING(0 0, 1 1, 2 0)'),
          (8, 'SRID=4326;CURVEPOLYGON(CIRCULARSTRING(0 0, 1 1, 2 0, 1 -1, 0 0))'),
          (9, 'SRID=4326;LINESTRING EMPTY'),
          (10, 'SRID=4326;GEOMETRYCOLLECTION(POINT(0 0), LINESTRING(0 0, 1 1))');
    "})
    .await;
    let kept = kept(
        &pool,
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
    assert_eq!(kept.by_sql, kept.by_plan);
    insta::assert_compact_debug_snapshot!(kept.by_sql, @"{3, 4, 7, 9}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_polygon_geometry() {
    let pool = connect(indoc! {r"
        CREATE TEMP TABLE where_rows (id int8, geom geometry);
        INSERT INTO where_rows VALUES
          (1, 'SRID=4326;POINT(0 0)'),
          (2, 'SRID=4326;MULTIPOINT(0 0, 1 1)'),
          (3, 'SRID=4326;LINESTRING(0 0, 1 1)'),
          (4, 'SRID=4326;MULTILINESTRING((0 0, 1 1))'),
          (5, 'SRID=4326;POLYGON((0 0, 1 0, 1 1, 0 0))'),
          (6, 'SRID=4326;MULTIPOLYGON(((0 0, 1 0, 1 1, 0 0)))'),
          (7, 'SRID=4326;CIRCULARSTRING(0 0, 1 1, 2 0)'),
          (8, 'SRID=4326;CURVEPOLYGON(CIRCULARSTRING(0 0, 1 1, 2 0, 1 -1, 0 0))'),
          (9, 'SRID=4326;LINESTRING EMPTY'),
          (10, 'SRID=4326;GEOMETRYCOLLECTION(POINT(0 0), LINESTRING(0 0, 1 1))');
    "})
    .await;
    let kept = kept(
        &pool,
        indoc! {r"
        schema: pg_temp
        table: where_rows
        srid: 4326
        geometry_column: geom
        geometry_type: GEOMETRY
        id_column: id
        layers:
          l:
            geometry: polygon
        "},
    )
    .await;
    assert_eq!(kept.by_sql, kept.by_plan);
    insta::assert_compact_debug_snapshot!(kept.by_sql, @"{5, 6, 8}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_bool_column() {
    let pool = connect(indoc! {r"
        CREATE TEMP TABLE where_rows (id int8, geom geometry(Geometry, 4326), b bool);
        INSERT INTO where_rows VALUES
          (1, 'SRID=4326;POINT(0 0)', true),
          (2, 'SRID=4326;LINESTRING(0 0, 1 1)', false),
          (3, 'SRID=4326;POLYGON((0 0, 1 0, 1 1, 0 0))', NULL),
          (4, 'SRID=4326;MULTIPOINT(0 0, 1 1)', true),
          (5, 'SRID=4326;POINT(1 1)', false),
          (6, 'SRID=4326;MULTIPOLYGON(((0 0, 1 0, 1 1, 0 0)))', NULL),
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', true),
          (8, 'SRID=4326;POINT(2 2)', false);
    "})
    .await;
    let kept = kept(
        &pool,
        indoc! {r"
        schema: pg_temp
        table: where_rows
        srid: 4326
        geometry_column: geom
        geometry_type: GEOMETRY
        id_column: id
        properties:
          b: bool
        layers:
          l:
            where: b
        "},
    )
    .await;
    assert_eq!(kept.by_sql, kept.by_plan);
    insta::assert_compact_debug_snapshot!(kept.by_sql, @"{1, 4, 7}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_negated_bool_column() {
    let pool = connect(indoc! {r"
        CREATE TEMP TABLE where_rows (id int8, geom geometry(Geometry, 4326), b bool);
        INSERT INTO where_rows VALUES
          (1, 'SRID=4326;POINT(0 0)', true),
          (2, 'SRID=4326;LINESTRING(0 0, 1 1)', false),
          (3, 'SRID=4326;POLYGON((0 0, 1 0, 1 1, 0 0))', NULL),
          (4, 'SRID=4326;MULTIPOINT(0 0, 1 1)', true),
          (5, 'SRID=4326;POINT(1 1)', false),
          (6, 'SRID=4326;MULTIPOLYGON(((0 0, 1 0, 1 1, 0 0)))', NULL),
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', true),
          (8, 'SRID=4326;POINT(2 2)', false);
    "})
    .await;
    let kept = kept(
        &pool,
        indoc! {r"
        schema: pg_temp
        table: where_rows
        srid: 4326
        geometry_column: geom
        geometry_type: GEOMETRY
        id_column: id
        properties:
          b: bool
        layers:
          l:
            where: '!b'
        "},
    )
    .await;
    assert_eq!(kept.by_sql, kept.by_plan);
    insta::assert_compact_debug_snapshot!(kept.by_sql, @"{2, 5, 8}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_bool_equals_true() {
    let pool = connect(indoc! {r"
        CREATE TEMP TABLE where_rows (id int8, geom geometry(Geometry, 4326), b bool);
        INSERT INTO where_rows VALUES
          (1, 'SRID=4326;POINT(0 0)', true),
          (2, 'SRID=4326;LINESTRING(0 0, 1 1)', false),
          (3, 'SRID=4326;POLYGON((0 0, 1 0, 1 1, 0 0))', NULL),
          (4, 'SRID=4326;MULTIPOINT(0 0, 1 1)', true),
          (5, 'SRID=4326;POINT(1 1)', false),
          (6, 'SRID=4326;MULTIPOLYGON(((0 0, 1 0, 1 1, 0 0)))', NULL),
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', true),
          (8, 'SRID=4326;POINT(2 2)', false);
    "})
    .await;
    let kept = kept(
        &pool,
        indoc! {r"
        schema: pg_temp
        table: where_rows
        srid: 4326
        geometry_column: geom
        geometry_type: GEOMETRY
        id_column: id
        properties:
          b: bool
        layers:
          l:
            where: 'b == true'
        "},
    )
    .await;
    assert_eq!(kept.by_sql, kept.by_plan);
    insta::assert_compact_debug_snapshot!(kept.by_sql, @"{1, 4, 7}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_bool_not_equals_true() {
    let pool = connect(indoc! {r"
        CREATE TEMP TABLE where_rows (id int8, geom geometry(Geometry, 4326), b bool);
        INSERT INTO where_rows VALUES
          (1, 'SRID=4326;POINT(0 0)', true),
          (2, 'SRID=4326;LINESTRING(0 0, 1 1)', false),
          (3, 'SRID=4326;POLYGON((0 0, 1 0, 1 1, 0 0))', NULL),
          (4, 'SRID=4326;MULTIPOINT(0 0, 1 1)', true),
          (5, 'SRID=4326;POINT(1 1)', false),
          (6, 'SRID=4326;MULTIPOLYGON(((0 0, 1 0, 1 1, 0 0)))', NULL),
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', true),
          (8, 'SRID=4326;POINT(2 2)', false);
    "})
    .await;
    let kept = kept(
        &pool,
        indoc! {r"
        schema: pg_temp
        table: where_rows
        srid: 4326
        geometry_column: geom
        geometry_type: GEOMETRY
        id_column: id
        properties:
          b: bool
        layers:
          l:
            where: 'b != true'
        "},
    )
    .await;
    assert_eq!(kept.by_sql, kept.by_plan);
    insta::assert_compact_debug_snapshot!(kept.by_sql, @"{2, 3, 5, 6, 8}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_bool_equals_null() {
    let pool = connect(indoc! {r"
        CREATE TEMP TABLE where_rows (id int8, geom geometry(Geometry, 4326), b bool);
        INSERT INTO where_rows VALUES
          (1, 'SRID=4326;POINT(0 0)', true),
          (2, 'SRID=4326;LINESTRING(0 0, 1 1)', false),
          (3, 'SRID=4326;POLYGON((0 0, 1 0, 1 1, 0 0))', NULL),
          (4, 'SRID=4326;MULTIPOINT(0 0, 1 1)', true),
          (5, 'SRID=4326;POINT(1 1)', false),
          (6, 'SRID=4326;MULTIPOLYGON(((0 0, 1 0, 1 1, 0 0)))', NULL),
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', true),
          (8, 'SRID=4326;POINT(2 2)', false);
    "})
    .await;
    let kept = kept(
        &pool,
        indoc! {r"
        schema: pg_temp
        table: where_rows
        srid: 4326
        geometry_column: geom
        geometry_type: GEOMETRY
        id_column: id
        properties:
          b: bool
        layers:
          l:
            where: 'b == null'
        "},
    )
    .await;
    assert_eq!(kept.by_sql, kept.by_plan);
    insta::assert_compact_debug_snapshot!(kept.by_sql, @"{3, 6}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_bool_not_equals_null() {
    let pool = connect(indoc! {r"
        CREATE TEMP TABLE where_rows (id int8, geom geometry(Geometry, 4326), b bool);
        INSERT INTO where_rows VALUES
          (1, 'SRID=4326;POINT(0 0)', true),
          (2, 'SRID=4326;LINESTRING(0 0, 1 1)', false),
          (3, 'SRID=4326;POLYGON((0 0, 1 0, 1 1, 0 0))', NULL),
          (4, 'SRID=4326;MULTIPOINT(0 0, 1 1)', true),
          (5, 'SRID=4326;POINT(1 1)', false),
          (6, 'SRID=4326;MULTIPOLYGON(((0 0, 1 0, 1 1, 0 0)))', NULL),
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', true),
          (8, 'SRID=4326;POINT(2 2)', false);
    "})
    .await;
    let kept = kept(
        &pool,
        indoc! {r"
        schema: pg_temp
        table: where_rows
        srid: 4326
        geometry_column: geom
        geometry_type: GEOMETRY
        id_column: id
        properties:
          b: bool
        layers:
          l:
            where: 'b != null'
        "},
    )
    .await;
    assert_eq!(kept.by_sql, kept.by_plan);
    insta::assert_compact_debug_snapshot!(kept.by_sql, @"{1, 2, 4, 5, 7, 8}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_int_greater() {
    let pool = connect(indoc! {r"
        CREATE TEMP TABLE where_rows (id int8, geom geometry(Geometry, 4326), i int4);
        INSERT INTO where_rows VALUES
          (1, 'SRID=4326;POINT(0 0)', 1),
          (2, 'SRID=4326;LINESTRING(0 0, 1 1)', -3),
          (3, 'SRID=4326;POLYGON((0 0, 1 0, 1 1, 0 0))', NULL),
          (4, 'SRID=4326;MULTIPOINT(0 0, 1 1)', 0),
          (5, 'SRID=4326;POINT(1 1)', 2147483647),
          (6, 'SRID=4326;MULTIPOLYGON(((0 0, 1 0, 1 1, 0 0)))', 3),
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', 2),
          (8, 'SRID=4326;POINT(2 2)', -1);
    "})
    .await;
    let kept = kept(
        &pool,
        indoc! {r"
        schema: pg_temp
        table: where_rows
        srid: 4326
        geometry_column: geom
        geometry_type: GEOMETRY
        id_column: id
        properties:
          i: int4
        layers:
          l:
            where: 'i > 1'
        "},
    )
    .await;
    assert_eq!(kept.by_sql, kept.by_plan);
    insta::assert_compact_debug_snapshot!(kept.by_sql, @"{5, 6, 7}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_negated_int_greater() {
    let pool = connect(indoc! {r"
        CREATE TEMP TABLE where_rows (id int8, geom geometry(Geometry, 4326), i int4);
        INSERT INTO where_rows VALUES
          (1, 'SRID=4326;POINT(0 0)', 1),
          (2, 'SRID=4326;LINESTRING(0 0, 1 1)', -3),
          (3, 'SRID=4326;POLYGON((0 0, 1 0, 1 1, 0 0))', NULL),
          (4, 'SRID=4326;MULTIPOINT(0 0, 1 1)', 0),
          (5, 'SRID=4326;POINT(1 1)', 2147483647),
          (6, 'SRID=4326;MULTIPOLYGON(((0 0, 1 0, 1 1, 0 0)))', 3),
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', 2),
          (8, 'SRID=4326;POINT(2 2)', -1);
    "})
    .await;
    let kept = kept(
        &pool,
        indoc! {r"
        schema: pg_temp
        table: where_rows
        srid: 4326
        geometry_column: geom
        geometry_type: GEOMETRY
        id_column: id
        properties:
          i: int4
        layers:
          l:
            where: '!(i > 1)'
        "},
    )
    .await;
    assert_eq!(kept.by_sql, kept.by_plan);
    insta::assert_compact_debug_snapshot!(kept.by_sql, @"{1, 2, 4, 8}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_int_at_most_float() {
    let pool = connect(indoc! {r"
        CREATE TEMP TABLE where_rows (id int8, geom geometry(Geometry, 4326), i int4);
        INSERT INTO where_rows VALUES
          (1, 'SRID=4326;POINT(0 0)', 1),
          (2, 'SRID=4326;LINESTRING(0 0, 1 1)', -3),
          (3, 'SRID=4326;POLYGON((0 0, 1 0, 1 1, 0 0))', NULL),
          (4, 'SRID=4326;MULTIPOINT(0 0, 1 1)', 0),
          (5, 'SRID=4326;POINT(1 1)', 2147483647),
          (6, 'SRID=4326;MULTIPOLYGON(((0 0, 1 0, 1 1, 0 0)))', 3),
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', 2),
          (8, 'SRID=4326;POINT(2 2)', -1);
    "})
    .await;
    let kept = kept(
        &pool,
        indoc! {r"
        schema: pg_temp
        table: where_rows
        srid: 4326
        geometry_column: geom
        geometry_type: GEOMETRY
        id_column: id
        properties:
          i: int4
        layers:
          l:
            where: 'i <= 1.5'
        "},
    )
    .await;
    assert_eq!(kept.by_sql, kept.by_plan);
    insta::assert_compact_debug_snapshot!(kept.by_sql, @"{1, 2, 4, 8}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_int_equals() {
    let pool = connect(indoc! {r"
        CREATE TEMP TABLE where_rows (id int8, geom geometry(Geometry, 4326), i int4);
        INSERT INTO where_rows VALUES
          (1, 'SRID=4326;POINT(0 0)', 1),
          (2, 'SRID=4326;LINESTRING(0 0, 1 1)', -3),
          (3, 'SRID=4326;POLYGON((0 0, 1 0, 1 1, 0 0))', NULL),
          (4, 'SRID=4326;MULTIPOINT(0 0, 1 1)', 0),
          (5, 'SRID=4326;POINT(1 1)', 2147483647),
          (6, 'SRID=4326;MULTIPOLYGON(((0 0, 1 0, 1 1, 0 0)))', 3),
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', 2),
          (8, 'SRID=4326;POINT(2 2)', -1);
    "})
    .await;
    let kept = kept(
        &pool,
        indoc! {r"
        schema: pg_temp
        table: where_rows
        srid: 4326
        geometry_column: geom
        geometry_type: GEOMETRY
        id_column: id
        properties:
          i: int4
        layers:
          l:
            where: 'i == 3'
        "},
    )
    .await;
    assert_eq!(kept.by_sql, kept.by_plan);
    insta::assert_compact_debug_snapshot!(kept.by_sql, @"{6}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_int_not_equals() {
    let pool = connect(indoc! {r"
        CREATE TEMP TABLE where_rows (id int8, geom geometry(Geometry, 4326), i int4);
        INSERT INTO where_rows VALUES
          (1, 'SRID=4326;POINT(0 0)', 1),
          (2, 'SRID=4326;LINESTRING(0 0, 1 1)', -3),
          (3, 'SRID=4326;POLYGON((0 0, 1 0, 1 1, 0 0))', NULL),
          (4, 'SRID=4326;MULTIPOINT(0 0, 1 1)', 0),
          (5, 'SRID=4326;POINT(1 1)', 2147483647),
          (6, 'SRID=4326;MULTIPOLYGON(((0 0, 1 0, 1 1, 0 0)))', 3),
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', 2),
          (8, 'SRID=4326;POINT(2 2)', -1);
    "})
    .await;
    let kept = kept(
        &pool,
        indoc! {r"
        schema: pg_temp
        table: where_rows
        srid: 4326
        geometry_column: geom
        geometry_type: GEOMETRY
        id_column: id
        properties:
          i: int4
        layers:
          l:
            where: 'i != 3'
        "},
    )
    .await;
    assert_eq!(kept.by_sql, kept.by_plan);
    insta::assert_compact_debug_snapshot!(kept.by_sql, @"{1, 2, 3, 4, 5, 7, 8}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_negative_literal_less_than_int() {
    let pool = connect(indoc! {r"
        CREATE TEMP TABLE where_rows (id int8, geom geometry(Geometry, 4326), i int4);
        INSERT INTO where_rows VALUES
          (1, 'SRID=4326;POINT(0 0)', 1),
          (2, 'SRID=4326;LINESTRING(0 0, 1 1)', -3),
          (3, 'SRID=4326;POLYGON((0 0, 1 0, 1 1, 0 0))', NULL),
          (4, 'SRID=4326;MULTIPOINT(0 0, 1 1)', 0),
          (5, 'SRID=4326;POINT(1 1)', 2147483647),
          (6, 'SRID=4326;MULTIPOLYGON(((0 0, 1 0, 1 1, 0 0)))', 3),
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', 2),
          (8, 'SRID=4326;POINT(2 2)', -1);
    "})
    .await;
    let kept = kept(
        &pool,
        indoc! {r"
        schema: pg_temp
        table: where_rows
        srid: 4326
        geometry_column: geom
        geometry_type: GEOMETRY
        id_column: id
        properties:
          i: int4
        layers:
          l:
            where: '-1 < i'
        "},
    )
    .await;
    assert_eq!(kept.by_sql, kept.by_plan);
    insta::assert_compact_debug_snapshot!(kept.by_sql, @"{1, 4, 5, 6, 7}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_float_greater() {
    let pool = connect(indoc! {r"
        CREATE TEMP TABLE where_rows (id int8, geom geometry(Geometry, 4326), f float8);
        INSERT INTO where_rows VALUES
          (1, 'SRID=4326;POINT(0 0)', 1.5),
          (2, 'SRID=4326;LINESTRING(0 0, 1 1)', 'NaN'),
          (3, 'SRID=4326;POLYGON((0 0, 1 0, 1 1, 0 0))', NULL),
          (4, 'SRID=4326;MULTIPOINT(0 0, 1 1)', -0.0),
          (5, 'SRID=4326;POINT(1 1)', 'Infinity'),
          (6, 'SRID=4326;MULTIPOLYGON(((0 0, 1 0, 1 1, 0 0)))', 2),
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', '-Infinity'),
          (8, 'SRID=4326;POINT(2 2)', 3.25);
    "})
    .await;
    let kept = kept(
        &pool,
        indoc! {r"
        schema: pg_temp
        table: where_rows
        srid: 4326
        geometry_column: geom
        geometry_type: GEOMETRY
        id_column: id
        properties:
          f: float8
        layers:
          l:
            where: 'f > 1.0'
        "},
    )
    .await;
    assert_eq!(kept.by_sql, kept.by_plan);
    insta::assert_compact_debug_snapshot!(kept.by_sql, @"{1, 5, 6, 8}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_negated_float_greater() {
    let pool = connect(indoc! {r"
        CREATE TEMP TABLE where_rows (id int8, geom geometry(Geometry, 4326), f float8);
        INSERT INTO where_rows VALUES
          (1, 'SRID=4326;POINT(0 0)', 1.5),
          (2, 'SRID=4326;LINESTRING(0 0, 1 1)', 'NaN'),
          (3, 'SRID=4326;POLYGON((0 0, 1 0, 1 1, 0 0))', NULL),
          (4, 'SRID=4326;MULTIPOINT(0 0, 1 1)', -0.0),
          (5, 'SRID=4326;POINT(1 1)', 'Infinity'),
          (6, 'SRID=4326;MULTIPOLYGON(((0 0, 1 0, 1 1, 0 0)))', 2),
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', '-Infinity'),
          (8, 'SRID=4326;POINT(2 2)', 3.25);
    "})
    .await;
    let kept = kept(
        &pool,
        indoc! {r"
        schema: pg_temp
        table: where_rows
        srid: 4326
        geometry_column: geom
        geometry_type: GEOMETRY
        id_column: id
        properties:
          f: float8
        layers:
          l:
            where: '!(f > 1.0)'
        "},
    )
    .await;
    assert_eq!(kept.by_sql, kept.by_plan);
    insta::assert_compact_debug_snapshot!(kept.by_sql, @"{4, 7}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_float_equals_int() {
    let pool = connect(indoc! {r"
        CREATE TEMP TABLE where_rows (id int8, geom geometry(Geometry, 4326), f float8);
        INSERT INTO where_rows VALUES
          (1, 'SRID=4326;POINT(0 0)', 1.5),
          (2, 'SRID=4326;LINESTRING(0 0, 1 1)', 'NaN'),
          (3, 'SRID=4326;POLYGON((0 0, 1 0, 1 1, 0 0))', NULL),
          (4, 'SRID=4326;MULTIPOINT(0 0, 1 1)', -0.0),
          (5, 'SRID=4326;POINT(1 1)', 'Infinity'),
          (6, 'SRID=4326;MULTIPOLYGON(((0 0, 1 0, 1 1, 0 0)))', 2),
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', '-Infinity'),
          (8, 'SRID=4326;POINT(2 2)', 3.25);
    "})
    .await;
    let kept = kept(
        &pool,
        indoc! {r"
        schema: pg_temp
        table: where_rows
        srid: 4326
        geometry_column: geom
        geometry_type: GEOMETRY
        id_column: id
        properties:
          f: float8
        layers:
          l:
            where: 'f == 2'
        "},
    )
    .await;
    assert_eq!(kept.by_sql, kept.by_plan);
    insta::assert_compact_debug_snapshot!(kept.by_sql, @"{6}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_float_not_equals_int() {
    let pool = connect(indoc! {r"
        CREATE TEMP TABLE where_rows (id int8, geom geometry(Geometry, 4326), f float8);
        INSERT INTO where_rows VALUES
          (1, 'SRID=4326;POINT(0 0)', 1.5),
          (2, 'SRID=4326;LINESTRING(0 0, 1 1)', 'NaN'),
          (3, 'SRID=4326;POLYGON((0 0, 1 0, 1 1, 0 0))', NULL),
          (4, 'SRID=4326;MULTIPOINT(0 0, 1 1)', -0.0),
          (5, 'SRID=4326;POINT(1 1)', 'Infinity'),
          (6, 'SRID=4326;MULTIPOLYGON(((0 0, 1 0, 1 1, 0 0)))', 2),
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', '-Infinity'),
          (8, 'SRID=4326;POINT(2 2)', 3.25);
    "})
    .await;
    let kept = kept(
        &pool,
        indoc! {r"
        schema: pg_temp
        table: where_rows
        srid: 4326
        geometry_column: geom
        geometry_type: GEOMETRY
        id_column: id
        properties:
          f: float8
        layers:
          l:
            where: 'f != 2'
        "},
    )
    .await;
    assert_eq!(kept.by_sql, kept.by_plan);
    insta::assert_compact_debug_snapshot!(kept.by_sql, @"{1, 2, 3, 4, 5, 7, 8}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_float_less_than_int() {
    let pool = connect(indoc! {r"
        CREATE TEMP TABLE where_rows (id int8, geom geometry(Geometry, 4326), f float8);
        INSERT INTO where_rows VALUES
          (1, 'SRID=4326;POINT(0 0)', 1.5),
          (2, 'SRID=4326;LINESTRING(0 0, 1 1)', 'NaN'),
          (3, 'SRID=4326;POLYGON((0 0, 1 0, 1 1, 0 0))', NULL),
          (4, 'SRID=4326;MULTIPOINT(0 0, 1 1)', -0.0),
          (5, 'SRID=4326;POINT(1 1)', 'Infinity'),
          (6, 'SRID=4326;MULTIPOLYGON(((0 0, 1 0, 1 1, 0 0)))', 2),
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', '-Infinity'),
          (8, 'SRID=4326;POINT(2 2)', 3.25);
    "})
    .await;
    let kept = kept(
        &pool,
        indoc! {r"
        schema: pg_temp
        table: where_rows
        srid: 4326
        geometry_column: geom
        geometry_type: GEOMETRY
        id_column: id
        properties:
          f: float8
        layers:
          l:
            where: 'f < 0'
        "},
    )
    .await;
    assert_eq!(kept.by_sql, kept.by_plan);
    insta::assert_compact_debug_snapshot!(kept.by_sql, @"{7}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_negated_float_less_than_int() {
    let pool = connect(indoc! {r"
        CREATE TEMP TABLE where_rows (id int8, geom geometry(Geometry, 4326), f float8);
        INSERT INTO where_rows VALUES
          (1, 'SRID=4326;POINT(0 0)', 1.5),
          (2, 'SRID=4326;LINESTRING(0 0, 1 1)', 'NaN'),
          (3, 'SRID=4326;POLYGON((0 0, 1 0, 1 1, 0 0))', NULL),
          (4, 'SRID=4326;MULTIPOINT(0 0, 1 1)', -0.0),
          (5, 'SRID=4326;POINT(1 1)', 'Infinity'),
          (6, 'SRID=4326;MULTIPOLYGON(((0 0, 1 0, 1 1, 0 0)))', 2),
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', '-Infinity'),
          (8, 'SRID=4326;POINT(2 2)', 3.25);
    "})
    .await;
    let kept = kept(
        &pool,
        indoc! {r"
        schema: pg_temp
        table: where_rows
        srid: 4326
        geometry_column: geom
        geometry_type: GEOMETRY
        id_column: id
        properties:
          f: float8
        layers:
          l:
            where: '!(f < 0)'
        "},
    )
    .await;
    assert_eq!(kept.by_sql, kept.by_plan);
    insta::assert_compact_debug_snapshot!(kept.by_sql, @"{1, 4, 5, 6, 8}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_int_equals_float_column() {
    let pool = connect(indoc! {r"
        CREATE TEMP TABLE where_rows (id int8, geom geometry(Geometry, 4326), i int4, f float8);
        INSERT INTO where_rows VALUES
          (1, 'SRID=4326;POINT(0 0)', 1, 1.5),
          (2, 'SRID=4326;LINESTRING(0 0, 1 1)', -3, 'NaN'),
          (3, 'SRID=4326;POLYGON((0 0, 1 0, 1 1, 0 0))', NULL, NULL),
          (4, 'SRID=4326;MULTIPOINT(0 0, 1 1)', 0, -0.0),
          (5, 'SRID=4326;POINT(1 1)', 2147483647, 'Infinity'),
          (6, 'SRID=4326;MULTIPOLYGON(((0 0, 1 0, 1 1, 0 0)))', 3, 2),
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', 2, '-Infinity'),
          (8, 'SRID=4326;POINT(2 2)', -1, 3.25);
    "})
    .await;
    let kept = kept(
        &pool,
        indoc! {r"
        schema: pg_temp
        table: where_rows
        srid: 4326
        geometry_column: geom
        geometry_type: GEOMETRY
        id_column: id
        properties:
          i: int4
          f: float8
        layers:
          l:
            where: 'i == f'
        "},
    )
    .await;
    assert_eq!(kept.by_sql, kept.by_plan);
    insta::assert_compact_debug_snapshot!(kept.by_sql, @"{3, 4}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_int_not_equals_float_column() {
    let pool = connect(indoc! {r"
        CREATE TEMP TABLE where_rows (id int8, geom geometry(Geometry, 4326), i int4, f float8);
        INSERT INTO where_rows VALUES
          (1, 'SRID=4326;POINT(0 0)', 1, 1.5),
          (2, 'SRID=4326;LINESTRING(0 0, 1 1)', -3, 'NaN'),
          (3, 'SRID=4326;POLYGON((0 0, 1 0, 1 1, 0 0))', NULL, NULL),
          (4, 'SRID=4326;MULTIPOINT(0 0, 1 1)', 0, -0.0),
          (5, 'SRID=4326;POINT(1 1)', 2147483647, 'Infinity'),
          (6, 'SRID=4326;MULTIPOLYGON(((0 0, 1 0, 1 1, 0 0)))', 3, 2),
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', 2, '-Infinity'),
          (8, 'SRID=4326;POINT(2 2)', -1, 3.25);
    "})
    .await;
    let kept = kept(
        &pool,
        indoc! {r"
        schema: pg_temp
        table: where_rows
        srid: 4326
        geometry_column: geom
        geometry_type: GEOMETRY
        id_column: id
        properties:
          i: int4
          f: float8
        layers:
          l:
            where: 'i != f'
        "},
    )
    .await;
    assert_eq!(kept.by_sql, kept.by_plan);
    insta::assert_compact_debug_snapshot!(kept.by_sql, @"{1, 2, 5, 6, 7, 8}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_string_equals() {
    let pool = connect(indoc! {r"
        CREATE TEMP TABLE where_rows (id int8, geom geometry(Geometry, 4326), s text);
        INSERT INTO where_rows VALUES
          (1, 'SRID=4326;POINT(0 0)', 'abc'),
          (2, 'SRID=4326;LINESTRING(0 0, 1 1)', 'a%_c'),
          (3, 'SRID=4326;POLYGON((0 0, 1 0, 1 1, 0 0))', NULL),
          (4, 'SRID=4326;MULTIPOINT(0 0, 1 1)', 'ABC'),
          (5, 'SRID=4326;POINT(1 1)', 'Übel'),
          (6, 'SRID=4326;MULTIPOLYGON(((0 0, 1 0, 1 1, 0 0)))', ''),
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', 'a\b'),
          (8, 'SRID=4326;POINT(2 2)', 'abd');
    "})
    .await;
    let kept = kept(
        &pool,
        indoc! {r#"
        schema: pg_temp
        table: where_rows
        srid: 4326
        geometry_column: geom
        geometry_type: GEOMETRY
        id_column: id
        properties:
          s: text
        layers:
          l:
            where: "s == 'abc'"
        "#},
    )
    .await;
    assert_eq!(kept.by_sql, kept.by_plan);
    insta::assert_compact_debug_snapshot!(kept.by_sql, @"{1}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_string_not_equals() {
    let pool = connect(indoc! {r"
        CREATE TEMP TABLE where_rows (id int8, geom geometry(Geometry, 4326), s text);
        INSERT INTO where_rows VALUES
          (1, 'SRID=4326;POINT(0 0)', 'abc'),
          (2, 'SRID=4326;LINESTRING(0 0, 1 1)', 'a%_c'),
          (3, 'SRID=4326;POLYGON((0 0, 1 0, 1 1, 0 0))', NULL),
          (4, 'SRID=4326;MULTIPOINT(0 0, 1 1)', 'ABC'),
          (5, 'SRID=4326;POINT(1 1)', 'Übel'),
          (6, 'SRID=4326;MULTIPOLYGON(((0 0, 1 0, 1 1, 0 0)))', ''),
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', 'a\b'),
          (8, 'SRID=4326;POINT(2 2)', 'abd');
    "})
    .await;
    let kept = kept(
        &pool,
        indoc! {r#"
        schema: pg_temp
        table: where_rows
        srid: 4326
        geometry_column: geom
        geometry_type: GEOMETRY
        id_column: id
        properties:
          s: text
        layers:
          l:
            where: "s != 'abc'"
        "#},
    )
    .await;
    assert_eq!(kept.by_sql, kept.by_plan);
    insta::assert_compact_debug_snapshot!(kept.by_sql, @"{2, 3, 4, 5, 6, 7, 8}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_string_less() {
    let pool = connect(indoc! {r"
        CREATE TEMP TABLE where_rows (id int8, geom geometry(Geometry, 4326), s text);
        INSERT INTO where_rows VALUES
          (1, 'SRID=4326;POINT(0 0)', 'abc'),
          (2, 'SRID=4326;LINESTRING(0 0, 1 1)', 'a%_c'),
          (3, 'SRID=4326;POLYGON((0 0, 1 0, 1 1, 0 0))', NULL),
          (4, 'SRID=4326;MULTIPOINT(0 0, 1 1)', 'ABC'),
          (5, 'SRID=4326;POINT(1 1)', 'Übel'),
          (6, 'SRID=4326;MULTIPOLYGON(((0 0, 1 0, 1 1, 0 0)))', ''),
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', 'a\b'),
          (8, 'SRID=4326;POINT(2 2)', 'abd');
    "})
    .await;
    let kept = kept(
        &pool,
        indoc! {r#"
        schema: pg_temp
        table: where_rows
        srid: 4326
        geometry_column: geom
        geometry_type: GEOMETRY
        id_column: id
        properties:
          s: text
        layers:
          l:
            where: "s < 'abd'"
        "#},
    )
    .await;
    assert_eq!(kept.by_sql, kept.by_plan);
    insta::assert_compact_debug_snapshot!(kept.by_sql, @"{1, 2, 4, 6, 7}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_negated_string_less() {
    let pool = connect(indoc! {r"
        CREATE TEMP TABLE where_rows (id int8, geom geometry(Geometry, 4326), s text);
        INSERT INTO where_rows VALUES
          (1, 'SRID=4326;POINT(0 0)', 'abc'),
          (2, 'SRID=4326;LINESTRING(0 0, 1 1)', 'a%_c'),
          (3, 'SRID=4326;POLYGON((0 0, 1 0, 1 1, 0 0))', NULL),
          (4, 'SRID=4326;MULTIPOINT(0 0, 1 1)', 'ABC'),
          (5, 'SRID=4326;POINT(1 1)', 'Übel'),
          (6, 'SRID=4326;MULTIPOLYGON(((0 0, 1 0, 1 1, 0 0)))', ''),
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', 'a\b'),
          (8, 'SRID=4326;POINT(2 2)', 'abd');
    "})
    .await;
    let kept = kept(
        &pool,
        indoc! {r#"
        schema: pg_temp
        table: where_rows
        srid: 4326
        geometry_column: geom
        geometry_type: GEOMETRY
        id_column: id
        properties:
          s: text
        layers:
          l:
            where: "!(s < 'abd')"
        "#},
    )
    .await;
    assert_eq!(kept.by_sql, kept.by_plan);
    insta::assert_compact_debug_snapshot!(kept.by_sql, @"{5, 8}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_string_at_least() {
    let pool = connect(indoc! {r"
        CREATE TEMP TABLE where_rows (id int8, geom geometry(Geometry, 4326), s text);
        INSERT INTO where_rows VALUES
          (1, 'SRID=4326;POINT(0 0)', 'abc'),
          (2, 'SRID=4326;LINESTRING(0 0, 1 1)', 'a%_c'),
          (3, 'SRID=4326;POLYGON((0 0, 1 0, 1 1, 0 0))', NULL),
          (4, 'SRID=4326;MULTIPOINT(0 0, 1 1)', 'ABC'),
          (5, 'SRID=4326;POINT(1 1)', 'Übel'),
          (6, 'SRID=4326;MULTIPOLYGON(((0 0, 1 0, 1 1, 0 0)))', ''),
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', 'a\b'),
          (8, 'SRID=4326;POINT(2 2)', 'abd');
    "})
    .await;
    let kept = kept(
        &pool,
        indoc! {r#"
        schema: pg_temp
        table: where_rows
        srid: 4326
        geometry_column: geom
        geometry_type: GEOMETRY
        id_column: id
        properties:
          s: text
        layers:
          l:
            where: "s >= 'a'"
        "#},
    )
    .await;
    assert_eq!(kept.by_sql, kept.by_plan);
    insta::assert_compact_debug_snapshot!(kept.by_sql, @"{1, 2, 5, 7, 8}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_date_equals() {
    let pool = connect(indoc! {r"
        CREATE TEMP TABLE where_rows (id int8, geom geometry(Geometry, 4326), d date);
        INSERT INTO where_rows VALUES
          (1, 'SRID=4326;POINT(0 0)', '2020-01-01'),
          (2, 'SRID=4326;LINESTRING(0 0, 1 1)', NULL),
          (3, 'SRID=4326;POLYGON((0 0, 1 0, 1 1, 0 0))', NULL),
          (4, 'SRID=4326;MULTIPOINT(0 0, 1 1)', '1999-12-31'),
          (5, 'SRID=4326;POINT(1 1)', NULL),
          (6, 'SRID=4326;MULTIPOLYGON(((0 0, 1 0, 1 1, 0 0)))', '2020-01-01'),
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', NULL),
          (8, 'SRID=4326;POINT(2 2)', NULL);
    "})
    .await;
    let kept = kept(
        &pool,
        indoc! {r#"
        schema: pg_temp
        table: where_rows
        srid: 4326
        geometry_column: geom
        geometry_type: GEOMETRY
        id_column: id
        properties:
          d: date
        layers:
          l:
            where: "d == '2020-01-01'"
        "#},
    )
    .await;
    assert_eq!(kept.by_sql, kept.by_plan);
    insta::assert_compact_debug_snapshot!(kept.by_sql, @"{1, 6}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_date_greater() {
    let pool = connect(indoc! {r"
        CREATE TEMP TABLE where_rows (id int8, geom geometry(Geometry, 4326), d date);
        INSERT INTO where_rows VALUES
          (1, 'SRID=4326;POINT(0 0)', '2020-01-01'),
          (2, 'SRID=4326;LINESTRING(0 0, 1 1)', NULL),
          (3, 'SRID=4326;POLYGON((0 0, 1 0, 1 1, 0 0))', NULL),
          (4, 'SRID=4326;MULTIPOINT(0 0, 1 1)', '1999-12-31'),
          (5, 'SRID=4326;POINT(1 1)', NULL),
          (6, 'SRID=4326;MULTIPOLYGON(((0 0, 1 0, 1 1, 0 0)))', '2020-01-01'),
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', NULL),
          (8, 'SRID=4326;POINT(2 2)', NULL);
    "})
    .await;
    let kept = kept(
        &pool,
        indoc! {r#"
        schema: pg_temp
        table: where_rows
        srid: 4326
        geometry_column: geom
        geometry_type: GEOMETRY
        id_column: id
        properties:
          d: date
        layers:
          l:
            where: "d > '2000'"
        "#},
    )
    .await;
    assert_eq!(kept.by_sql, kept.by_plan);
    insta::assert_compact_debug_snapshot!(kept.by_sql, @"{1, 6}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_string_equals_int() {
    let pool = connect(indoc! {r"
        CREATE TEMP TABLE where_rows (id int8, geom geometry(Geometry, 4326), s text);
        INSERT INTO where_rows VALUES
          (1, 'SRID=4326;POINT(0 0)', 'abc'),
          (2, 'SRID=4326;LINESTRING(0 0, 1 1)', 'a%_c'),
          (3, 'SRID=4326;POLYGON((0 0, 1 0, 1 1, 0 0))', NULL),
          (4, 'SRID=4326;MULTIPOINT(0 0, 1 1)', 'ABC'),
          (5, 'SRID=4326;POINT(1 1)', 'Übel'),
          (6, 'SRID=4326;MULTIPOLYGON(((0 0, 1 0, 1 1, 0 0)))', ''),
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', 'a\b'),
          (8, 'SRID=4326;POINT(2 2)', 'abd');
    "})
    .await;
    let kept = kept(
        &pool,
        indoc! {r"
        schema: pg_temp
        table: where_rows
        srid: 4326
        geometry_column: geom
        geometry_type: GEOMETRY
        id_column: id
        properties:
          s: text
        layers:
          l:
            where: 's == 1'
        "},
    )
    .await;
    assert_eq!(kept.by_sql, kept.by_plan);
    insta::assert_compact_debug_snapshot!(kept.by_sql, @"{}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_string_not_equals_int() {
    let pool = connect(indoc! {r"
        CREATE TEMP TABLE where_rows (id int8, geom geometry(Geometry, 4326), s text);
        INSERT INTO where_rows VALUES
          (1, 'SRID=4326;POINT(0 0)', 'abc'),
          (2, 'SRID=4326;LINESTRING(0 0, 1 1)', 'a%_c'),
          (3, 'SRID=4326;POLYGON((0 0, 1 0, 1 1, 0 0))', NULL),
          (4, 'SRID=4326;MULTIPOINT(0 0, 1 1)', 'ABC'),
          (5, 'SRID=4326;POINT(1 1)', 'Übel'),
          (6, 'SRID=4326;MULTIPOLYGON(((0 0, 1 0, 1 1, 0 0)))', ''),
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', 'a\b'),
          (8, 'SRID=4326;POINT(2 2)', 'abd');
    "})
    .await;
    let kept = kept(
        &pool,
        indoc! {r"
        schema: pg_temp
        table: where_rows
        srid: 4326
        geometry_column: geom
        geometry_type: GEOMETRY
        id_column: id
        properties:
          s: text
        layers:
          l:
            where: 's != 1'
        "},
    )
    .await;
    assert_eq!(kept.by_sql, kept.by_plan);
    insta::assert_compact_debug_snapshot!(kept.by_sql, @"{1, 2, 3, 4, 5, 6, 7, 8}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_int_equals_string() {
    let pool = connect(indoc! {r"
        CREATE TEMP TABLE where_rows (id int8, geom geometry(Geometry, 4326), i int4);
        INSERT INTO where_rows VALUES
          (1, 'SRID=4326;POINT(0 0)', 1),
          (2, 'SRID=4326;LINESTRING(0 0, 1 1)', -3),
          (3, 'SRID=4326;POLYGON((0 0, 1 0, 1 1, 0 0))', NULL),
          (4, 'SRID=4326;MULTIPOINT(0 0, 1 1)', 0),
          (5, 'SRID=4326;POINT(1 1)', 2147483647),
          (6, 'SRID=4326;MULTIPOLYGON(((0 0, 1 0, 1 1, 0 0)))', 3),
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', 2),
          (8, 'SRID=4326;POINT(2 2)', -1);
    "})
    .await;
    let kept = kept(
        &pool,
        indoc! {r#"
        schema: pg_temp
        table: where_rows
        srid: 4326
        geometry_column: geom
        geometry_type: GEOMETRY
        id_column: id
        properties:
          i: int4
        layers:
          l:
            where: "i == 'x'"
        "#},
    )
    .await;
    assert_eq!(kept.by_sql, kept.by_plan);
    insta::assert_compact_debug_snapshot!(kept.by_sql, @"{}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_string_greater_than_int() {
    let pool = connect(indoc! {r"
        CREATE TEMP TABLE where_rows (id int8, geom geometry(Geometry, 4326), s text);
        INSERT INTO where_rows VALUES
          (1, 'SRID=4326;POINT(0 0)', 'abc'),
          (2, 'SRID=4326;LINESTRING(0 0, 1 1)', 'a%_c'),
          (3, 'SRID=4326;POLYGON((0 0, 1 0, 1 1, 0 0))', NULL),
          (4, 'SRID=4326;MULTIPOINT(0 0, 1 1)', 'ABC'),
          (5, 'SRID=4326;POINT(1 1)', 'Übel'),
          (6, 'SRID=4326;MULTIPOLYGON(((0 0, 1 0, 1 1, 0 0)))', ''),
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', 'a\b'),
          (8, 'SRID=4326;POINT(2 2)', 'abd');
    "})
    .await;
    let kept = kept(
        &pool,
        indoc! {r"
        schema: pg_temp
        table: where_rows
        srid: 4326
        geometry_column: geom
        geometry_type: GEOMETRY
        id_column: id
        properties:
          s: text
        layers:
          l:
            where: 's > 1'
        "},
    )
    .await;
    assert_eq!(kept.by_sql, kept.by_plan);
    insta::assert_compact_debug_snapshot!(kept.by_sql, @"{}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_negated_string_greater_than_int() {
    let pool = connect(indoc! {r"
        CREATE TEMP TABLE where_rows (id int8, geom geometry(Geometry, 4326), s text);
        INSERT INTO where_rows VALUES
          (1, 'SRID=4326;POINT(0 0)', 'abc'),
          (2, 'SRID=4326;LINESTRING(0 0, 1 1)', 'a%_c'),
          (3, 'SRID=4326;POLYGON((0 0, 1 0, 1 1, 0 0))', NULL),
          (4, 'SRID=4326;MULTIPOINT(0 0, 1 1)', 'ABC'),
          (5, 'SRID=4326;POINT(1 1)', 'Übel'),
          (6, 'SRID=4326;MULTIPOLYGON(((0 0, 1 0, 1 1, 0 0)))', ''),
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', 'a\b'),
          (8, 'SRID=4326;POINT(2 2)', 'abd');
    "})
    .await;
    let kept = kept(
        &pool,
        indoc! {r"
        schema: pg_temp
        table: where_rows
        srid: 4326
        geometry_column: geom
        geometry_type: GEOMETRY
        id_column: id
        properties:
          s: text
        layers:
          l:
            where: '!(s > 1)'
        "},
    )
    .await;
    assert_eq!(kept.by_sql, kept.by_plan);
    insta::assert_compact_debug_snapshot!(kept.by_sql, @"{}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_bool_equals_string_column() {
    let pool = connect(indoc! {r"
        CREATE TEMP TABLE where_rows (id int8, geom geometry(Geometry, 4326), b bool, s text);
        INSERT INTO where_rows VALUES
          (1, 'SRID=4326;POINT(0 0)', true, 'abc'),
          (2, 'SRID=4326;LINESTRING(0 0, 1 1)', false, 'a%_c'),
          (3, 'SRID=4326;POLYGON((0 0, 1 0, 1 1, 0 0))', NULL, NULL),
          (4, 'SRID=4326;MULTIPOINT(0 0, 1 1)', true, 'ABC'),
          (5, 'SRID=4326;POINT(1 1)', false, 'Übel'),
          (6, 'SRID=4326;MULTIPOLYGON(((0 0, 1 0, 1 1, 0 0)))', NULL, ''),
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', true, 'a\b'),
          (8, 'SRID=4326;POINT(2 2)', false, 'abd');
    "})
    .await;
    let kept = kept(
        &pool,
        indoc! {r"
        schema: pg_temp
        table: where_rows
        srid: 4326
        geometry_column: geom
        geometry_type: GEOMETRY
        id_column: id
        properties:
          b: bool
          s: text
        layers:
          l:
            where: 'b == s'
        "},
    )
    .await;
    assert_eq!(kept.by_sql, kept.by_plan);
    insta::assert_compact_debug_snapshot!(kept.by_sql, @"{3}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_bool_not_equals_string_column() {
    let pool = connect(indoc! {r"
        CREATE TEMP TABLE where_rows (id int8, geom geometry(Geometry, 4326), b bool, s text);
        INSERT INTO where_rows VALUES
          (1, 'SRID=4326;POINT(0 0)', true, 'abc'),
          (2, 'SRID=4326;LINESTRING(0 0, 1 1)', false, 'a%_c'),
          (3, 'SRID=4326;POLYGON((0 0, 1 0, 1 1, 0 0))', NULL, NULL),
          (4, 'SRID=4326;MULTIPOINT(0 0, 1 1)', true, 'ABC'),
          (5, 'SRID=4326;POINT(1 1)', false, 'Übel'),
          (6, 'SRID=4326;MULTIPOLYGON(((0 0, 1 0, 1 1, 0 0)))', NULL, ''),
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', true, 'a\b'),
          (8, 'SRID=4326;POINT(2 2)', false, 'abd');
    "})
    .await;
    let kept = kept(
        &pool,
        indoc! {r"
        schema: pg_temp
        table: where_rows
        srid: 4326
        geometry_column: geom
        geometry_type: GEOMETRY
        id_column: id
        properties:
          b: bool
          s: text
        layers:
          l:
            where: 'b != s'
        "},
    )
    .await;
    assert_eq!(kept.by_sql, kept.by_plan);
    insta::assert_compact_debug_snapshot!(kept.by_sql, @"{1, 2, 4, 5, 6, 7, 8}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_and() {
    let pool = connect(indoc! {r"
        CREATE TEMP TABLE where_rows (id int8, geom geometry(Geometry, 4326), b bool, i int4);
        INSERT INTO where_rows VALUES
          (1, 'SRID=4326;POINT(0 0)', true, 1),
          (2, 'SRID=4326;LINESTRING(0 0, 1 1)', false, -3),
          (3, 'SRID=4326;POLYGON((0 0, 1 0, 1 1, 0 0))', NULL, NULL),
          (4, 'SRID=4326;MULTIPOINT(0 0, 1 1)', true, 0),
          (5, 'SRID=4326;POINT(1 1)', false, 2147483647),
          (6, 'SRID=4326;MULTIPOLYGON(((0 0, 1 0, 1 1, 0 0)))', NULL, 3),
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', true, 2),
          (8, 'SRID=4326;POINT(2 2)', false, -1);
    "})
    .await;
    let kept = kept(
        &pool,
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
        layers:
          l:
            where: 'b && i > 0'
        "},
    )
    .await;
    assert_eq!(kept.by_sql, kept.by_plan);
    insta::assert_compact_debug_snapshot!(kept.by_sql, @"{1, 7}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_negated_and() {
    let pool = connect(indoc! {r"
        CREATE TEMP TABLE where_rows (id int8, geom geometry(Geometry, 4326), b bool, i int4);
        INSERT INTO where_rows VALUES
          (1, 'SRID=4326;POINT(0 0)', true, 1),
          (2, 'SRID=4326;LINESTRING(0 0, 1 1)', false, -3),
          (3, 'SRID=4326;POLYGON((0 0, 1 0, 1 1, 0 0))', NULL, NULL),
          (4, 'SRID=4326;MULTIPOINT(0 0, 1 1)', true, 0),
          (5, 'SRID=4326;POINT(1 1)', false, 2147483647),
          (6, 'SRID=4326;MULTIPOLYGON(((0 0, 1 0, 1 1, 0 0)))', NULL, 3),
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', true, 2),
          (8, 'SRID=4326;POINT(2 2)', false, -1);
    "})
    .await;
    let kept = kept(
        &pool,
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
        layers:
          l:
            where: '!(b && i > 0)'
        "},
    )
    .await;
    assert_eq!(kept.by_sql, kept.by_plan);
    insta::assert_compact_debug_snapshot!(kept.by_sql, @"{2, 4, 5, 8}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_or() {
    let pool = connect(indoc! {r"
        CREATE TEMP TABLE where_rows (id int8, geom geometry(Geometry, 4326), b bool, s text);
        INSERT INTO where_rows VALUES
          (1, 'SRID=4326;POINT(0 0)', true, 'abc'),
          (2, 'SRID=4326;LINESTRING(0 0, 1 1)', false, 'a%_c'),
          (3, 'SRID=4326;POLYGON((0 0, 1 0, 1 1, 0 0))', NULL, NULL),
          (4, 'SRID=4326;MULTIPOINT(0 0, 1 1)', true, 'ABC'),
          (5, 'SRID=4326;POINT(1 1)', false, 'Übel'),
          (6, 'SRID=4326;MULTIPOLYGON(((0 0, 1 0, 1 1, 0 0)))', NULL, ''),
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', true, 'a\b'),
          (8, 'SRID=4326;POINT(2 2)', false, 'abd');
    "})
    .await;
    let kept = kept(
        &pool,
        indoc! {r#"
        schema: pg_temp
        table: where_rows
        srid: 4326
        geometry_column: geom
        geometry_type: GEOMETRY
        id_column: id
        properties:
          b: bool
          s: text
        layers:
          l:
            where: "b || s == ''"
        "#},
    )
    .await;
    assert_eq!(kept.by_sql, kept.by_plan);
    insta::assert_compact_debug_snapshot!(kept.by_sql, @"{1, 4, 6, 7}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_negated_or() {
    let pool = connect(indoc! {r"
        CREATE TEMP TABLE where_rows (id int8, geom geometry(Geometry, 4326), b bool, f float8);
        INSERT INTO where_rows VALUES
          (1, 'SRID=4326;POINT(0 0)', true, 1.5),
          (2, 'SRID=4326;LINESTRING(0 0, 1 1)', false, 'NaN'),
          (3, 'SRID=4326;POLYGON((0 0, 1 0, 1 1, 0 0))', NULL, NULL),
          (4, 'SRID=4326;MULTIPOINT(0 0, 1 1)', true, -0.0),
          (5, 'SRID=4326;POINT(1 1)', false, 'Infinity'),
          (6, 'SRID=4326;MULTIPOLYGON(((0 0, 1 0, 1 1, 0 0)))', NULL, 2),
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', true, '-Infinity'),
          (8, 'SRID=4326;POINT(2 2)', false, 3.25);
    "})
    .await;
    let kept = kept(
        &pool,
        indoc! {r"
        schema: pg_temp
        table: where_rows
        srid: 4326
        geometry_column: geom
        geometry_type: GEOMETRY
        id_column: id
        properties:
          b: bool
          f: float8
        layers:
          l:
            where: '!(b || f > 1.0)'
        "},
    )
    .await;
    assert_eq!(kept.by_sql, kept.by_plan);
    insta::assert_compact_debug_snapshot!(kept.by_sql, @"{}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_conditional() {
    let pool = connect(indoc! {r"
        CREATE TEMP TABLE where_rows (
          id int8, geom geometry(Geometry, 4326), b bool, i int4, s text
        );
        INSERT INTO where_rows VALUES
          (1, 'SRID=4326;POINT(0 0)', true, 1, 'abc'),
          (2, 'SRID=4326;LINESTRING(0 0, 1 1)', false, -3, 'a%_c'),
          (3, 'SRID=4326;POLYGON((0 0, 1 0, 1 1, 0 0))', NULL, NULL, NULL),
          (4, 'SRID=4326;MULTIPOINT(0 0, 1 1)', true, 0, 'ABC'),
          (5, 'SRID=4326;POINT(1 1)', false, 2147483647, 'Übel'),
          (6, 'SRID=4326;MULTIPOLYGON(((0 0, 1 0, 1 1, 0 0)))', NULL, 3, ''),
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', true, 2, 'a\b'),
          (8, 'SRID=4326;POINT(2 2)', false, -1, 'abd');
    "})
    .await;
    let kept = kept(
        &pool,
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
          s: text
        layers:
          l:
            where: "b ? i > 0 : s == ''"
        "#},
    )
    .await;
    assert_eq!(kept.by_sql, kept.by_plan);
    insta::assert_compact_debug_snapshot!(kept.by_sql, @"{1, 7}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_negated_conditional() {
    let pool = connect(indoc! {r"
        CREATE TEMP TABLE where_rows (
          id int8, geom geometry(Geometry, 4326), b bool, i int4, s text
        );
        INSERT INTO where_rows VALUES
          (1, 'SRID=4326;POINT(0 0)', true, 1, 'abc'),
          (2, 'SRID=4326;LINESTRING(0 0, 1 1)', false, -3, 'a%_c'),
          (3, 'SRID=4326;POLYGON((0 0, 1 0, 1 1, 0 0))', NULL, NULL, NULL),
          (4, 'SRID=4326;MULTIPOINT(0 0, 1 1)', true, 0, 'ABC'),
          (5, 'SRID=4326;POINT(1 1)', false, 2147483647, 'Übel'),
          (6, 'SRID=4326;MULTIPOLYGON(((0 0, 1 0, 1 1, 0 0)))', NULL, 3, ''),
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', true, 2, 'a\b'),
          (8, 'SRID=4326;POINT(2 2)', false, -1, 'abd');
    "})
    .await;
    let kept = kept(
        &pool,
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
          s: text
        layers:
          l:
            where: "!(b ? i > 0 : s == '')"
        "#},
    )
    .await;
    assert_eq!(kept.by_sql, kept.by_plan);
    insta::assert_compact_debug_snapshot!(kept.by_sql, @"{2, 4, 5, 8}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_false_literal() {
    let pool = connect(indoc! {r"
        CREATE TEMP TABLE where_rows (id int8, geom geometry(Geometry, 4326));
        INSERT INTO where_rows VALUES
          (1, 'SRID=4326;POINT(0 0)'),
          (2, 'SRID=4326;LINESTRING(0 0, 1 1)'),
          (3, 'SRID=4326;POLYGON((0 0, 1 0, 1 1, 0 0))'),
          (4, 'SRID=4326;MULTIPOINT(0 0, 1 1)'),
          (5, 'SRID=4326;POINT(1 1)'),
          (6, 'SRID=4326;MULTIPOLYGON(((0 0, 1 0, 1 1, 0 0)))'),
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))'),
          (8, 'SRID=4326;POINT(2 2)');
    "})
    .await;
    let kept = kept(
        &pool,
        indoc! {r"
        schema: pg_temp
        table: where_rows
        srid: 4326
        geometry_column: geom
        geometry_type: GEOMETRY
        id_column: id
        layers:
          l:
            where: 'false'
        "},
    )
    .await;
    assert_eq!(kept.by_sql, kept.by_plan);
    insta::assert_compact_debug_snapshot!(kept.by_sql, @"{}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_polygon_geometry_and_where() {
    let pool = connect(indoc! {r"
        CREATE TEMP TABLE where_rows (id int8, geom geometry(Geometry, 4326), b bool);
        INSERT INTO where_rows VALUES
          (1, 'SRID=4326;POINT(0 0)', true),
          (2, 'SRID=4326;LINESTRING(0 0, 1 1)', false),
          (3, 'SRID=4326;POLYGON((0 0, 1 0, 1 1, 0 0))', NULL),
          (4, 'SRID=4326;MULTIPOINT(0 0, 1 1)', true),
          (5, 'SRID=4326;POINT(1 1)', false),
          (6, 'SRID=4326;MULTIPOLYGON(((0 0, 1 0, 1 1, 0 0)))', NULL),
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', true),
          (8, 'SRID=4326;POINT(2 2)', false);
    "})
    .await;
    let kept = kept(
        &pool,
        indoc! {r"
        schema: pg_temp
        table: where_rows
        srid: 4326
        geometry_column: geom
        geometry_type: GEOMETRY
        id_column: id
        properties:
          b: bool
        layers:
          l:
            geometry: polygon
            where: 'b != true'
        "},
    )
    .await;
    assert_eq!(kept.by_sql, kept.by_plan);
    insta::assert_compact_debug_snapshot!(kept.by_sql, @"{3, 6}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_string_equals_under_a_nondeterministic_collation() {
    let pool = connect(indoc! {r"
        CREATE COLLATION pg_temp.ci (provider = icu, locale = 'und-u-ks-level2', deterministic = false);
        CREATE TEMP TABLE where_rows (
          id int8, geom geometry(Geometry, 4326), s text COLLATE pg_temp.ci
        );
        INSERT INTO where_rows VALUES
          (1, 'SRID=4326;POINT(0 0)', 'abc'),
          (2, 'SRID=4326;POINT(1 1)', 'ABC'),
          (3, 'SRID=4326;POINT(2 2)', NULL),
          (4, 'SRID=4326;POINT(3 3)', 'abd');
    "})
    .await;
    let kept = kept(
        &pool,
        indoc! {r#"
        schema: pg_temp
        table: where_rows
        srid: 4326
        geometry_column: geom
        geometry_type: GEOMETRY
        id_column: id
        properties:
          s: text
        layers:
          l:
            where: "s == 'abc'"
        "#},
    )
    .await;
    assert_eq!(kept.by_sql, kept.by_plan);
    insta::assert_compact_debug_snapshot!(kept.by_sql, @"{1}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_string_not_equals_under_a_nondeterministic_collation() {
    let pool = connect(indoc! {r"
        CREATE COLLATION pg_temp.ci (provider = icu, locale = 'und-u-ks-level2', deterministic = false);
        CREATE TEMP TABLE where_rows (
          id int8, geom geometry(Geometry, 4326), s text COLLATE pg_temp.ci
        );
        INSERT INTO where_rows VALUES
          (1, 'SRID=4326;POINT(0 0)', 'abc'),
          (2, 'SRID=4326;POINT(1 1)', 'ABC'),
          (3, 'SRID=4326;POINT(2 2)', NULL),
          (4, 'SRID=4326;POINT(3 3)', 'abd');
    "})
    .await;
    let kept = kept(
        &pool,
        indoc! {r#"
        schema: pg_temp
        table: where_rows
        srid: 4326
        geometry_column: geom
        geometry_type: GEOMETRY
        id_column: id
        properties:
          s: text
        layers:
          l:
            where: "s != 'abc'"
        "#},
    )
    .await;
    assert_eq!(kept.by_sql, kept.by_plan);
    insta::assert_compact_debug_snapshot!(kept.by_sql, @"{2, 3, 4}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_string_columns_equal_under_a_nondeterministic_collation() {
    let pool = connect(indoc! {r"
        CREATE COLLATION pg_temp.ci (provider = icu, locale = 'und-u-ks-level2', deterministic = false);
        CREATE TEMP TABLE where_rows (
          id int8, geom geometry(Geometry, 4326), s text COLLATE pg_temp.ci, t text COLLATE pg_temp.ci
        );
        INSERT INTO where_rows VALUES
          (1, 'SRID=4326;POINT(0 0)', 'abc', 'abc'),
          (2, 'SRID=4326;POINT(1 1)', 'ABC', 'abc'),
          (3, 'SRID=4326;POINT(2 2)', NULL, 'abc'),
          (4, 'SRID=4326;POINT(3 3)', 'abd', NULL);
    "})
    .await;
    let kept = kept(
        &pool,
        indoc! {r#"
        schema: pg_temp
        table: where_rows
        srid: 4326
        geometry_column: geom
        geometry_type: GEOMETRY
        id_column: id
        properties:
          s: text
          t: text
        layers:
          l:
            where: "s == t"
        "#},
    )
    .await;
    assert_eq!(kept.by_sql, kept.by_plan);
    insta::assert_compact_debug_snapshot!(kept.by_sql, @"{1}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_string_columns_not_equal_under_a_nondeterministic_collation() {
    let pool = connect(indoc! {r"
        CREATE COLLATION pg_temp.ci (provider = icu, locale = 'und-u-ks-level2', deterministic = false);
        CREATE TEMP TABLE where_rows (
          id int8, geom geometry(Geometry, 4326), s text COLLATE pg_temp.ci, t text COLLATE pg_temp.ci
        );
        INSERT INTO where_rows VALUES
          (1, 'SRID=4326;POINT(0 0)', 'abc', 'abc'),
          (2, 'SRID=4326;POINT(1 1)', 'ABC', 'abc'),
          (3, 'SRID=4326;POINT(2 2)', NULL, 'abc'),
          (4, 'SRID=4326;POINT(3 3)', 'abd', NULL);
    "})
    .await;
    let kept = kept(
        &pool,
        indoc! {r#"
        schema: pg_temp
        table: where_rows
        srid: 4326
        geometry_column: geom
        geometry_type: GEOMETRY
        id_column: id
        properties:
          s: text
          t: text
        layers:
          l:
            where: "s != t"
        "#},
    )
    .await;
    assert_eq!(kept.by_sql, kept.by_plan);
    insta::assert_compact_debug_snapshot!(kept.by_sql, @"{2, 3, 4}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_varchar_equals() {
    let pool = connect(indoc! {r"
        CREATE TEMP TABLE where_rows (id int8, geom geometry(Geometry, 4326), v varchar(5));
        INSERT INTO where_rows VALUES
          (1, 'SRID=4326;POINT(0 0)', 'a'),
          (2, 'SRID=4326;POINT(1 1)', 'B'),
          (3, 'SRID=4326;POINT(2 2)', 'b'),
          (4, 'SRID=4326;POINT(3 3)', NULL),
          (5, 'SRID=4326;POINT(4 4)', 'Übel');
    "})
    .await;
    let kept = kept(
        &pool,
        indoc! {r#"
        schema: pg_temp
        table: where_rows
        srid: 4326
        geometry_column: geom
        geometry_type: GEOMETRY
        id_column: id
        properties:
          v: varchar
        layers:
          l:
            where: "v == 'b'"
        "#},
    )
    .await;
    assert_eq!(kept.by_sql, kept.by_plan);
    insta::assert_compact_debug_snapshot!(kept.by_sql, @"{3}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_varchar_less() {
    let pool = connect(indoc! {r"
        CREATE TEMP TABLE where_rows (id int8, geom geometry(Geometry, 4326), v varchar(5));
        INSERT INTO where_rows VALUES
          (1, 'SRID=4326;POINT(0 0)', 'a'),
          (2, 'SRID=4326;POINT(1 1)', 'B'),
          (3, 'SRID=4326;POINT(2 2)', 'b'),
          (4, 'SRID=4326;POINT(3 3)', NULL),
          (5, 'SRID=4326;POINT(4 4)', 'Übel');
    "})
    .await;
    let kept = kept(
        &pool,
        indoc! {r#"
        schema: pg_temp
        table: where_rows
        srid: 4326
        geometry_column: geom
        geometry_type: GEOMETRY
        id_column: id
        properties:
          v: varchar
        layers:
          l:
            where: "v < 'b'"
        "#},
    )
    .await;
    assert_eq!(kept.by_sql, kept.by_plan);
    insta::assert_compact_debug_snapshot!(kept.by_sql, @"{1, 2}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_int2_greater() {
    let pool = connect(indoc! {r"
        CREATE TEMP TABLE where_rows (id int8, geom geometry(Geometry, 4326), h int2);
        INSERT INTO where_rows VALUES
          (1, 'SRID=4326;POINT(0 0)', 1),
          (2, 'SRID=4326;POINT(1 1)', -32768),
          (3, 'SRID=4326;POINT(2 2)', 32767),
          (4, 'SRID=4326;POINT(3 3)', NULL),
          (5, 'SRID=4326;POINT(4 4)', 2);
    "})
    .await;
    let kept = kept(
        &pool,
        indoc! {r#"
        schema: pg_temp
        table: where_rows
        srid: 4326
        geometry_column: geom
        geometry_type: GEOMETRY
        id_column: id
        properties:
          h: int2
        layers:
          l:
            where: "h > 1"
        "#},
    )
    .await;
    assert_eq!(kept.by_sql, kept.by_plan);
    insta::assert_compact_debug_snapshot!(kept.by_sql, @"{3, 5}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_float4_greater() {
    let pool = connect(indoc! {r"
        CREATE TEMP TABLE where_rows (id int8, geom geometry(Geometry, 4326), r float4);
        INSERT INTO where_rows VALUES
          (1, 'SRID=4326;POINT(0 0)', 1.1),
          (2, 'SRID=4326;POINT(1 1)', 2.5),
          (3, 'SRID=4326;POINT(2 2)', 'NaN'),
          (4, 'SRID=4326;POINT(3 3)', NULL),
          (5, 'SRID=4326;POINT(4 4)', '-Infinity');
    "})
    .await;
    let kept = kept(
        &pool,
        indoc! {r#"
        schema: pg_temp
        table: where_rows
        srid: 4326
        geometry_column: geom
        geometry_type: GEOMETRY
        id_column: id
        properties:
          r: float4
        layers:
          l:
            where: "r > 1.1"
        "#},
    )
    .await;
    assert_eq!(kept.by_sql, kept.by_plan);
    insta::assert_compact_debug_snapshot!(kept.by_sql, @"{1, 2}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_float4_equals() {
    let pool = connect(indoc! {r"
        CREATE TEMP TABLE where_rows (id int8, geom geometry(Geometry, 4326), r float4);
        INSERT INTO where_rows VALUES
          (1, 'SRID=4326;POINT(0 0)', 1.1),
          (2, 'SRID=4326;POINT(1 1)', 2.5),
          (3, 'SRID=4326;POINT(2 2)', 'NaN'),
          (4, 'SRID=4326;POINT(3 3)', NULL),
          (5, 'SRID=4326;POINT(4 4)', '-Infinity');
    "})
    .await;
    let kept = kept(
        &pool,
        indoc! {r#"
        schema: pg_temp
        table: where_rows
        srid: 4326
        geometry_column: geom
        geometry_type: GEOMETRY
        id_column: id
        properties:
          r: float4
        layers:
          l:
            where: "r == 1.1"
        "#},
    )
    .await;
    assert_eq!(kept.by_sql, kept.by_plan);
    insta::assert_compact_debug_snapshot!(kept.by_sql, @"{}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_int8_equals_float_past_2_pow_53() {
    let pool = connect(indoc! {r"
        CREATE TEMP TABLE where_rows (id int8, geom geometry(Geometry, 4326), n int8);
        INSERT INTO where_rows VALUES
          (1, 'SRID=4326;POINT(0 0)', 9007199254740992),
          (2, 'SRID=4326;POINT(1 1)', 9007199254740993),
          (3, 'SRID=4326;POINT(2 2)', -9223372036854775808),
          (4, 'SRID=4326;POINT(3 3)', NULL),
          (5, 'SRID=4326;POINT(4 4)', 9223372036854775807);
    "})
    .await;
    let kept = kept(
        &pool,
        indoc! {r#"
        schema: pg_temp
        table: where_rows
        srid: 4326
        geometry_column: geom
        geometry_type: GEOMETRY
        id_column: id
        properties:
          n: int8
        layers:
          l:
            where: "n == 9007199254740993.0"
        "#},
    )
    .await;
    assert_eq!(kept.by_sql, kept.by_plan);
    insta::assert_compact_debug_snapshot!(kept.by_sql, @"{1, 2}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_int8_greater_than_float_past_2_pow_53() {
    let pool = connect(indoc! {r"
        CREATE TEMP TABLE where_rows (id int8, geom geometry(Geometry, 4326), n int8);
        INSERT INTO where_rows VALUES
          (1, 'SRID=4326;POINT(0 0)', 9007199254740992),
          (2, 'SRID=4326;POINT(1 1)', 9007199254740993),
          (3, 'SRID=4326;POINT(2 2)', -9223372036854775808),
          (4, 'SRID=4326;POINT(3 3)', NULL),
          (5, 'SRID=4326;POINT(4 4)', 9223372036854775807);
    "})
    .await;
    let kept = kept(
        &pool,
        indoc! {r#"
        schema: pg_temp
        table: where_rows
        srid: 4326
        geometry_column: geom
        geometry_type: GEOMETRY
        id_column: id
        properties:
          n: int8
        layers:
          l:
            where: "n > 9007199254740992.0"
        "#},
    )
    .await;
    assert_eq!(kept.by_sql, kept.by_plan);
    insta::assert_compact_debug_snapshot!(kept.by_sql, @"{5}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_int_less_than_uint() {
    let pool = connect(indoc! {r"
        CREATE TEMP TABLE where_rows (id int8, geom geometry(Geometry, 4326), h int2);
        INSERT INTO where_rows VALUES
          (1, 'SRID=4326;POINT(0 0)', 1),
          (2, 'SRID=4326;POINT(1 1)', -32768),
          (3, 'SRID=4326;POINT(2 2)', 32767),
          (4, 'SRID=4326;POINT(3 3)', NULL),
          (5, 'SRID=4326;POINT(4 4)', 2);
    "})
    .await;
    let kept = kept(
        &pool,
        indoc! {r#"
        schema: pg_temp
        table: where_rows
        srid: 4326
        geometry_column: geom
        geometry_type: GEOMETRY
        id_column: id
        properties:
          h: int2
        layers:
          l:
            where: "h < 2u"
        "#},
    )
    .await;
    assert_eq!(kept.by_sql, kept.by_plan);
    insta::assert_compact_debug_snapshot!(kept.by_sql, @"{1, 2}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_string_columns_equal_under_different_collations() {
    let pool = connect(indoc! {r#"
        CREATE TEMP TABLE where_rows (
          id int8, geom geometry(Geometry, 4326), a text COLLATE "C", b text COLLATE "en-x-icu"
        );
        INSERT INTO where_rows VALUES
          (1, 'SRID=4326;POINT(0 0)', 'x', 'x'),
          (2, 'SRID=4326;POINT(1 1)', 'x', 'X'),
          (3, 'SRID=4326;POINT(2 2)', NULL, 'x'),
          (4, 'SRID=4326;POINT(3 3)', NULL, NULL);
    "#})
    .await;
    let kept = kept(
        &pool,
        indoc! {r#"
        schema: pg_temp
        table: where_rows
        srid: 4326
        geometry_column: geom
        geometry_type: GEOMETRY
        id_column: id
        properties:
          a: text
          b: text
        layers:
          l:
            where: "a == b"
        "#},
    )
    .await;
    assert_eq!(kept.by_sql, kept.by_plan);
    insta::assert_compact_debug_snapshot!(kept.by_sql, @"{1, 4}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_string_columns_not_equal_under_different_collations() {
    let pool = connect(indoc! {r#"
        CREATE TEMP TABLE where_rows (
          id int8, geom geometry(Geometry, 4326), a text COLLATE "C", b text COLLATE "en-x-icu"
        );
        INSERT INTO where_rows VALUES
          (1, 'SRID=4326;POINT(0 0)', 'x', 'x'),
          (2, 'SRID=4326;POINT(1 1)', 'x', 'X'),
          (3, 'SRID=4326;POINT(2 2)', NULL, 'x'),
          (4, 'SRID=4326;POINT(3 3)', NULL, NULL);
    "#})
    .await;
    let kept = kept(
        &pool,
        indoc! {r#"
        schema: pg_temp
        table: where_rows
        srid: 4326
        geometry_column: geom
        geometry_type: GEOMETRY
        id_column: id
        properties:
          a: text
          b: text
        layers:
          l:
            where: "a != b"
        "#},
    )
    .await;
    assert_eq!(kept.by_sql, kept.by_plan);
    insta::assert_compact_debug_snapshot!(kept.by_sql, @"{2, 3}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_int_in_list_with_null() {
    let client = connect(indoc! {r"
        CREATE TEMP TABLE where_rows (id int8, geom geometry(Geometry, 4326), i int4);
        INSERT INTO where_rows VALUES
          (1, 'SRID=4326;POINT(0 0)', 1),
          (2, 'SRID=4326;LINESTRING(0 0, 1 1)', -3),
          (3, 'SRID=4326;POLYGON((0 0, 1 0, 1 1, 0 0))', NULL),
          (4, 'SRID=4326;MULTIPOINT(0 0, 1 1)', 0),
          (5, 'SRID=4326;POINT(1 1)', 2147483647),
          (6, 'SRID=4326;MULTIPOLYGON(((0 0, 1 0, 1 1, 0 0)))', 3),
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', 2),
          (8, 'SRID=4326;POINT(2 2)', -1);
    "})
    .await;
    let kept = kept(
        &client,
        indoc! {r"
        schema: pg_temp
        table: where_rows
        srid: 4326
        geometry_column: geom
        geometry_type: GEOMETRY
        id_column: id
        properties:
          i: int4
        layers:
          l:
            where: 'i in [1, 2, null]'
        "},
    )
    .await;
    assert_eq!(kept.by_sql, kept.by_plan);
    insta::assert_compact_debug_snapshot!(kept.by_sql, @"{1, 3, 7}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_int_not_in_list() {
    let client = connect(indoc! {r"
        CREATE TEMP TABLE where_rows (id int8, geom geometry(Geometry, 4326), i int4);
        INSERT INTO where_rows VALUES
          (1, 'SRID=4326;POINT(0 0)', 1),
          (2, 'SRID=4326;LINESTRING(0 0, 1 1)', -3),
          (3, 'SRID=4326;POLYGON((0 0, 1 0, 1 1, 0 0))', NULL),
          (4, 'SRID=4326;MULTIPOINT(0 0, 1 1)', 0),
          (5, 'SRID=4326;POINT(1 1)', 2147483647),
          (6, 'SRID=4326;MULTIPOLYGON(((0 0, 1 0, 1 1, 0 0)))', 3),
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', 2),
          (8, 'SRID=4326;POINT(2 2)', -1);
    "})
    .await;
    let kept = kept(
        &client,
        indoc! {r"
        schema: pg_temp
        table: where_rows
        srid: 4326
        geometry_column: geom
        geometry_type: GEOMETRY
        id_column: id
        properties:
          i: int4
        layers:
          l:
            where: '!(i in [1, 2])'
        "},
    )
    .await;
    assert_eq!(kept.by_sql, kept.by_plan);
    insta::assert_compact_debug_snapshot!(kept.by_sql, @"{2, 3, 4, 5, 6, 8}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_int_not_in_list_with_null() {
    let client = connect(indoc! {r"
        CREATE TEMP TABLE where_rows (id int8, geom geometry(Geometry, 4326), i int4);
        INSERT INTO where_rows VALUES
          (1, 'SRID=4326;POINT(0 0)', 1),
          (2, 'SRID=4326;LINESTRING(0 0, 1 1)', -3),
          (3, 'SRID=4326;POLYGON((0 0, 1 0, 1 1, 0 0))', NULL),
          (4, 'SRID=4326;MULTIPOINT(0 0, 1 1)', 0),
          (5, 'SRID=4326;POINT(1 1)', 2147483647),
          (6, 'SRID=4326;MULTIPOLYGON(((0 0, 1 0, 1 1, 0 0)))', 3),
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', 2),
          (8, 'SRID=4326;POINT(2 2)', -1);
    "})
    .await;
    let kept = kept(
        &client,
        indoc! {r"
        schema: pg_temp
        table: where_rows
        srid: 4326
        geometry_column: geom
        geometry_type: GEOMETRY
        id_column: id
        properties:
          i: int4
        layers:
          l:
            where: '!(i in [1, null])'
        "},
    )
    .await;
    assert_eq!(kept.by_sql, kept.by_plan);
    insta::assert_compact_debug_snapshot!(kept.by_sql, @"{2, 4, 5, 6, 7, 8}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_int_in_empty_list() {
    let client = connect(indoc! {r"
        CREATE TEMP TABLE where_rows (id int8, geom geometry(Geometry, 4326), i int4);
        INSERT INTO where_rows VALUES
          (1, 'SRID=4326;POINT(0 0)', 1),
          (2, 'SRID=4326;LINESTRING(0 0, 1 1)', -3),
          (3, 'SRID=4326;POLYGON((0 0, 1 0, 1 1, 0 0))', NULL),
          (4, 'SRID=4326;MULTIPOINT(0 0, 1 1)', 0),
          (5, 'SRID=4326;POINT(1 1)', 2147483647),
          (6, 'SRID=4326;MULTIPOLYGON(((0 0, 1 0, 1 1, 0 0)))', 3),
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', 2),
          (8, 'SRID=4326;POINT(2 2)', -1);
    "})
    .await;
    let kept = kept(
        &client,
        indoc! {r"
        schema: pg_temp
        table: where_rows
        srid: 4326
        geometry_column: geom
        geometry_type: GEOMETRY
        id_column: id
        properties:
          i: int4
        layers:
          l:
            where: 'i in []'
        "},
    )
    .await;
    assert_eq!(kept.by_sql, kept.by_plan);
    insta::assert_compact_debug_snapshot!(kept.by_sql, @"{}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_int_not_in_empty_list() {
    let client = connect(indoc! {r"
        CREATE TEMP TABLE where_rows (id int8, geom geometry(Geometry, 4326), i int4);
        INSERT INTO where_rows VALUES
          (1, 'SRID=4326;POINT(0 0)', 1),
          (2, 'SRID=4326;LINESTRING(0 0, 1 1)', -3),
          (3, 'SRID=4326;POLYGON((0 0, 1 0, 1 1, 0 0))', NULL),
          (4, 'SRID=4326;MULTIPOINT(0 0, 1 1)', 0),
          (5, 'SRID=4326;POINT(1 1)', 2147483647),
          (6, 'SRID=4326;MULTIPOLYGON(((0 0, 1 0, 1 1, 0 0)))', 3),
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', 2),
          (8, 'SRID=4326;POINT(2 2)', -1);
    "})
    .await;
    let kept = kept(
        &client,
        indoc! {r"
        schema: pg_temp
        table: where_rows
        srid: 4326
        geometry_column: geom
        geometry_type: GEOMETRY
        id_column: id
        properties:
          i: int4
        layers:
          l:
            where: '!(i in [])'
        "},
    )
    .await;
    assert_eq!(kept.by_sql, kept.by_plan);
    insta::assert_compact_debug_snapshot!(kept.by_sql, @"{1, 2, 3, 4, 5, 6, 7, 8}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_float_in_list() {
    let client = connect(indoc! {r"
        CREATE TEMP TABLE where_rows (id int8, geom geometry(Geometry, 4326), f float8);
        INSERT INTO where_rows VALUES
          (1, 'SRID=4326;POINT(0 0)', 1.5),
          (2, 'SRID=4326;LINESTRING(0 0, 1 1)', 'NaN'),
          (3, 'SRID=4326;POLYGON((0 0, 1 0, 1 1, 0 0))', NULL),
          (4, 'SRID=4326;MULTIPOINT(0 0, 1 1)', -0.0),
          (5, 'SRID=4326;POINT(1 1)', 'Infinity'),
          (6, 'SRID=4326;MULTIPOLYGON(((0 0, 1 0, 1 1, 0 0)))', 2),
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', '-Infinity'),
          (8, 'SRID=4326;POINT(2 2)', 3.25);
    "})
    .await;
    let kept = kept(
        &client,
        indoc! {r"
        schema: pg_temp
        table: where_rows
        srid: 4326
        geometry_column: geom
        geometry_type: GEOMETRY
        id_column: id
        properties:
          f: float8
        layers:
          l:
            where: 'f in [1.5, 2]'
        "},
    )
    .await;
    assert_eq!(kept.by_sql, kept.by_plan);
    insta::assert_compact_debug_snapshot!(kept.by_sql, @"{1, 6}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_float_not_in_list() {
    let client = connect(indoc! {r"
        CREATE TEMP TABLE where_rows (id int8, geom geometry(Geometry, 4326), f float8);
        INSERT INTO where_rows VALUES
          (1, 'SRID=4326;POINT(0 0)', 1.5),
          (2, 'SRID=4326;LINESTRING(0 0, 1 1)', 'NaN'),
          (3, 'SRID=4326;POLYGON((0 0, 1 0, 1 1, 0 0))', NULL),
          (4, 'SRID=4326;MULTIPOINT(0 0, 1 1)', -0.0),
          (5, 'SRID=4326;POINT(1 1)', 'Infinity'),
          (6, 'SRID=4326;MULTIPOLYGON(((0 0, 1 0, 1 1, 0 0)))', 2),
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', '-Infinity'),
          (8, 'SRID=4326;POINT(2 2)', 3.25);
    "})
    .await;
    let kept = kept(
        &client,
        indoc! {r"
        schema: pg_temp
        table: where_rows
        srid: 4326
        geometry_column: geom
        geometry_type: GEOMETRY
        id_column: id
        properties:
          f: float8
        layers:
          l:
            where: '!(f in [1.5, 2])'
        "},
    )
    .await;
    assert_eq!(kept.by_sql, kept.by_plan);
    insta::assert_compact_debug_snapshot!(kept.by_sql, @"{2, 3, 4, 5, 7, 8}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_int8_in_list_of_float_and_int() {
    let client = connect(indoc! {r"
        CREATE TEMP TABLE where_rows (id int8, geom geometry(Geometry, 4326), n int8);
        INSERT INTO where_rows VALUES
          (1, 'SRID=4326;POINT(0 0)', 9007199254740992),
          (2, 'SRID=4326;LINESTRING(0 0, 1 1)', NULL),
          (3, 'SRID=4326;POLYGON((0 0, 1 0, 1 1, 0 0))', NULL),
          (4, 'SRID=4326;MULTIPOINT(0 0, 1 1)', NULL),
          (5, 'SRID=4326;POINT(1 1)', NULL),
          (6, 'SRID=4326;MULTIPOLYGON(((0 0, 1 0, 1 1, 0 0)))', NULL),
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', NULL),
          (8, 'SRID=4326;POINT(2 2)', NULL);
    "})
    .await;
    let kept = kept(
        &client,
        indoc! {r"
        schema: pg_temp
        table: where_rows
        srid: 4326
        geometry_column: geom
        geometry_type: GEOMETRY
        id_column: id
        properties:
          n: int8
        layers:
          l:
            where: 'n in [0.5, 9007199254740993]'
        "},
    )
    .await;
    assert_eq!(kept.by_sql, kept.by_plan);
    insta::assert_compact_debug_snapshot!(kept.by_sql, @"{}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_int8_in_list_of_int() {
    let client = connect(indoc! {r"
        CREATE TEMP TABLE where_rows (id int8, geom geometry(Geometry, 4326), n int8);
        INSERT INTO where_rows VALUES
          (1, 'SRID=4326;POINT(0 0)', 9007199254740992),
          (2, 'SRID=4326;LINESTRING(0 0, 1 1)', NULL),
          (3, 'SRID=4326;POLYGON((0 0, 1 0, 1 1, 0 0))', NULL),
          (4, 'SRID=4326;MULTIPOINT(0 0, 1 1)', NULL),
          (5, 'SRID=4326;POINT(1 1)', NULL),
          (6, 'SRID=4326;MULTIPOLYGON(((0 0, 1 0, 1 1, 0 0)))', NULL),
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', NULL),
          (8, 'SRID=4326;POINT(2 2)', NULL);
    "})
    .await;
    let kept = kept(
        &client,
        indoc! {r"
        schema: pg_temp
        table: where_rows
        srid: 4326
        geometry_column: geom
        geometry_type: GEOMETRY
        id_column: id
        properties:
          n: int8
        layers:
          l:
            where: 'n in [9007199254740992]'
        "},
    )
    .await;
    assert_eq!(kept.by_sql, kept.by_plan);
    insta::assert_compact_debug_snapshot!(kept.by_sql, @"{1}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_string_in_list() {
    let client = connect(indoc! {r"
        CREATE TEMP TABLE where_rows (id int8, geom geometry(Geometry, 4326), s text);
        INSERT INTO where_rows VALUES
          (1, 'SRID=4326;POINT(0 0)', 'abc'),
          (2, 'SRID=4326;LINESTRING(0 0, 1 1)', 'a%_c'),
          (3, 'SRID=4326;POLYGON((0 0, 1 0, 1 1, 0 0))', NULL),
          (4, 'SRID=4326;MULTIPOINT(0 0, 1 1)', 'ABC'),
          (5, 'SRID=4326;POINT(1 1)', 'Übel'),
          (6, 'SRID=4326;MULTIPOLYGON(((0 0, 1 0, 1 1, 0 0)))', ''),
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', 'a\b'),
          (8, 'SRID=4326;POINT(2 2)', 'abd');
    "})
    .await;
    let kept = kept(
        &client,
        indoc! {r#"
        schema: pg_temp
        table: where_rows
        srid: 4326
        geometry_column: geom
        geometry_type: GEOMETRY
        id_column: id
        properties:
          s: text
        layers:
          l:
            where: "s in ['abc', 'ABC']"
        "#},
    )
    .await;
    assert_eq!(kept.by_sql, kept.by_plan);
    insta::assert_compact_debug_snapshot!(kept.by_sql, @"{1, 4}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_string_not_in_list() {
    let client = connect(indoc! {r"
        CREATE TEMP TABLE where_rows (id int8, geom geometry(Geometry, 4326), s text);
        INSERT INTO where_rows VALUES
          (1, 'SRID=4326;POINT(0 0)', 'abc'),
          (2, 'SRID=4326;LINESTRING(0 0, 1 1)', 'a%_c'),
          (3, 'SRID=4326;POLYGON((0 0, 1 0, 1 1, 0 0))', NULL),
          (4, 'SRID=4326;MULTIPOINT(0 0, 1 1)', 'ABC'),
          (5, 'SRID=4326;POINT(1 1)', 'Übel'),
          (6, 'SRID=4326;MULTIPOLYGON(((0 0, 1 0, 1 1, 0 0)))', ''),
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', 'a\b'),
          (8, 'SRID=4326;POINT(2 2)', 'abd');
    "})
    .await;
    let kept = kept(
        &client,
        indoc! {r#"
        schema: pg_temp
        table: where_rows
        srid: 4326
        geometry_column: geom
        geometry_type: GEOMETRY
        id_column: id
        properties:
          s: text
        layers:
          l:
            where: "!(s in ['abc', 'ABC'])"
        "#},
    )
    .await;
    assert_eq!(kept.by_sql, kept.by_plan);
    insta::assert_compact_debug_snapshot!(kept.by_sql, @"{2, 3, 5, 6, 7, 8}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_starts_with_percent() {
    let client = connect(indoc! {r"
        CREATE TEMP TABLE where_rows (id int8, geom geometry(Geometry, 4326), s text);
        INSERT INTO where_rows VALUES
          (1, 'SRID=4326;POINT(0 0)', 'abc'),
          (2, 'SRID=4326;LINESTRING(0 0, 1 1)', 'a%_c'),
          (3, 'SRID=4326;POLYGON((0 0, 1 0, 1 1, 0 0))', NULL),
          (4, 'SRID=4326;MULTIPOINT(0 0, 1 1)', 'ABC'),
          (5, 'SRID=4326;POINT(1 1)', 'Übel'),
          (6, 'SRID=4326;MULTIPOLYGON(((0 0, 1 0, 1 1, 0 0)))', ''),
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', 'a\b'),
          (8, 'SRID=4326;POINT(2 2)', 'abd');
    "})
    .await;
    let kept = kept(
        &client,
        indoc! {r#"
        schema: pg_temp
        table: where_rows
        srid: 4326
        geometry_column: geom
        geometry_type: GEOMETRY
        id_column: id
        properties:
          s: text
        layers:
          l:
            where: "s.startsWith('a%')"
        "#},
    )
    .await;
    assert_eq!(kept.by_sql, kept.by_plan);
    insta::assert_compact_debug_snapshot!(kept.by_sql, @"{2}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_ends_with() {
    let client = connect(indoc! {r"
        CREATE TEMP TABLE where_rows (id int8, geom geometry(Geometry, 4326), s text);
        INSERT INTO where_rows VALUES
          (1, 'SRID=4326;POINT(0 0)', 'abc'),
          (2, 'SRID=4326;LINESTRING(0 0, 1 1)', 'a%_c'),
          (3, 'SRID=4326;POLYGON((0 0, 1 0, 1 1, 0 0))', NULL),
          (4, 'SRID=4326;MULTIPOINT(0 0, 1 1)', 'ABC'),
          (5, 'SRID=4326;POINT(1 1)', 'Übel'),
          (6, 'SRID=4326;MULTIPOLYGON(((0 0, 1 0, 1 1, 0 0)))', ''),
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', 'a\b'),
          (8, 'SRID=4326;POINT(2 2)', 'abd');
    "})
    .await;
    let kept = kept(
        &client,
        indoc! {r#"
        schema: pg_temp
        table: where_rows
        srid: 4326
        geometry_column: geom
        geometry_type: GEOMETRY
        id_column: id
        properties:
          s: text
        layers:
          l:
            where: "s.endsWith('c')"
        "#},
    )
    .await;
    assert_eq!(kept.by_sql, kept.by_plan);
    insta::assert_compact_debug_snapshot!(kept.by_sql, @"{1, 2}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_contains_underscore() {
    let client = connect(indoc! {r"
        CREATE TEMP TABLE where_rows (id int8, geom geometry(Geometry, 4326), s text);
        INSERT INTO where_rows VALUES
          (1, 'SRID=4326;POINT(0 0)', 'abc'),
          (2, 'SRID=4326;LINESTRING(0 0, 1 1)', 'a%_c'),
          (3, 'SRID=4326;POLYGON((0 0, 1 0, 1 1, 0 0))', NULL),
          (4, 'SRID=4326;MULTIPOINT(0 0, 1 1)', 'ABC'),
          (5, 'SRID=4326;POINT(1 1)', 'Übel'),
          (6, 'SRID=4326;MULTIPOLYGON(((0 0, 1 0, 1 1, 0 0)))', ''),
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', 'a\b'),
          (8, 'SRID=4326;POINT(2 2)', 'abd');
    "})
    .await;
    let kept = kept(
        &client,
        indoc! {r#"
        schema: pg_temp
        table: where_rows
        srid: 4326
        geometry_column: geom
        geometry_type: GEOMETRY
        id_column: id
        properties:
          s: text
        layers:
          l:
            where: "s.contains('_')"
        "#},
    )
    .await;
    assert_eq!(kept.by_sql, kept.by_plan);
    insta::assert_compact_debug_snapshot!(kept.by_sql, @"{2}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_contains_backslash() {
    let client = connect(indoc! {r"
        CREATE TEMP TABLE where_rows (id int8, geom geometry(Geometry, 4326), s text);
        INSERT INTO where_rows VALUES
          (1, 'SRID=4326;POINT(0 0)', 'abc'),
          (2, 'SRID=4326;LINESTRING(0 0, 1 1)', 'a%_c'),
          (3, 'SRID=4326;POLYGON((0 0, 1 0, 1 1, 0 0))', NULL),
          (4, 'SRID=4326;MULTIPOINT(0 0, 1 1)', 'ABC'),
          (5, 'SRID=4326;POINT(1 1)', 'Übel'),
          (6, 'SRID=4326;MULTIPOLYGON(((0 0, 1 0, 1 1, 0 0)))', ''),
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', 'a\b'),
          (8, 'SRID=4326;POINT(2 2)', 'abd');
    "})
    .await;
    let kept = kept(
        &client,
        indoc! {r#"
        schema: pg_temp
        table: where_rows
        srid: 4326
        geometry_column: geom
        geometry_type: GEOMETRY
        id_column: id
        properties:
          s: text
        layers:
          l:
            where: "s.contains('\\\\')"
        "#},
    )
    .await;
    assert_eq!(kept.by_sql, kept.by_plan);
    insta::assert_compact_debug_snapshot!(kept.by_sql, @"{7}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_negated_starts_with() {
    let client = connect(indoc! {r"
        CREATE TEMP TABLE where_rows (id int8, geom geometry(Geometry, 4326), s text);
        INSERT INTO where_rows VALUES
          (1, 'SRID=4326;POINT(0 0)', 'abc'),
          (2, 'SRID=4326;LINESTRING(0 0, 1 1)', 'a%_c'),
          (3, 'SRID=4326;POLYGON((0 0, 1 0, 1 1, 0 0))', NULL),
          (4, 'SRID=4326;MULTIPOINT(0 0, 1 1)', 'ABC'),
          (5, 'SRID=4326;POINT(1 1)', 'Übel'),
          (6, 'SRID=4326;MULTIPOLYGON(((0 0, 1 0, 1 1, 0 0)))', ''),
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', 'a\b'),
          (8, 'SRID=4326;POINT(2 2)', 'abd');
    "})
    .await;
    let kept = kept(
        &client,
        indoc! {r#"
        schema: pg_temp
        table: where_rows
        srid: 4326
        geometry_column: geom
        geometry_type: GEOMETRY
        id_column: id
        properties:
          s: text
        layers:
          l:
            where: "!s.startsWith('a')"
        "#},
    )
    .await;
    assert_eq!(kept.by_sql, kept.by_plan);
    insta::assert_compact_debug_snapshot!(kept.by_sql, @"{4, 5, 6}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_size_equals() {
    let client = connect(indoc! {r"
        CREATE TEMP TABLE where_rows (id int8, geom geometry(Geometry, 4326), s text);
        INSERT INTO where_rows VALUES
          (1, 'SRID=4326;POINT(0 0)', 'abc'),
          (2, 'SRID=4326;LINESTRING(0 0, 1 1)', 'a%_c'),
          (3, 'SRID=4326;POLYGON((0 0, 1 0, 1 1, 0 0))', NULL),
          (4, 'SRID=4326;MULTIPOINT(0 0, 1 1)', 'ABC'),
          (5, 'SRID=4326;POINT(1 1)', 'Übel'),
          (6, 'SRID=4326;MULTIPOLYGON(((0 0, 1 0, 1 1, 0 0)))', ''),
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', 'a\b'),
          (8, 'SRID=4326;POINT(2 2)', 'abd');
    "})
    .await;
    let kept = kept(
        &client,
        indoc! {r"
        schema: pg_temp
        table: where_rows
        srid: 4326
        geometry_column: geom
        geometry_type: GEOMETRY
        id_column: id
        properties:
          s: text
        layers:
          l:
            where: 'size(s) == 3'
        "},
    )
    .await;
    assert_eq!(kept.by_sql, kept.by_plan);
    insta::assert_compact_debug_snapshot!(kept.by_sql, @"{1, 4, 7, 8}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_size_not_equals() {
    let client = connect(indoc! {r"
        CREATE TEMP TABLE where_rows (id int8, geom geometry(Geometry, 4326), s text);
        INSERT INTO where_rows VALUES
          (1, 'SRID=4326;POINT(0 0)', 'abc'),
          (2, 'SRID=4326;LINESTRING(0 0, 1 1)', 'a%_c'),
          (3, 'SRID=4326;POLYGON((0 0, 1 0, 1 1, 0 0))', NULL),
          (4, 'SRID=4326;MULTIPOINT(0 0, 1 1)', 'ABC'),
          (5, 'SRID=4326;POINT(1 1)', 'Übel'),
          (6, 'SRID=4326;MULTIPOLYGON(((0 0, 1 0, 1 1, 0 0)))', ''),
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', 'a\b'),
          (8, 'SRID=4326;POINT(2 2)', 'abd');
    "})
    .await;
    let kept = kept(
        &client,
        indoc! {r"
        schema: pg_temp
        table: where_rows
        srid: 4326
        geometry_column: geom
        geometry_type: GEOMETRY
        id_column: id
        properties:
          s: text
        layers:
          l:
            where: 'size(s) != 3'
        "},
    )
    .await;
    assert_eq!(kept.by_sql, kept.by_plan);
    insta::assert_compact_debug_snapshot!(kept.by_sql, @"{2, 5, 6}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_size_greater() {
    let client = connect(indoc! {r"
        CREATE TEMP TABLE where_rows (id int8, geom geometry(Geometry, 4326), s text);
        INSERT INTO where_rows VALUES
          (1, 'SRID=4326;POINT(0 0)', 'abc'),
          (2, 'SRID=4326;LINESTRING(0 0, 1 1)', 'a%_c'),
          (3, 'SRID=4326;POLYGON((0 0, 1 0, 1 1, 0 0))', NULL),
          (4, 'SRID=4326;MULTIPOINT(0 0, 1 1)', 'ABC'),
          (5, 'SRID=4326;POINT(1 1)', 'Übel'),
          (6, 'SRID=4326;MULTIPOLYGON(((0 0, 1 0, 1 1, 0 0)))', ''),
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', 'a\b'),
          (8, 'SRID=4326;POINT(2 2)', 'abd');
    "})
    .await;
    let kept = kept(
        &client,
        indoc! {r"
        schema: pg_temp
        table: where_rows
        srid: 4326
        geometry_column: geom
        geometry_type: GEOMETRY
        id_column: id
        properties:
          s: text
        layers:
          l:
            where: 'size(s) > 2'
        "},
    )
    .await;
    assert_eq!(kept.by_sql, kept.by_plan);
    insta::assert_compact_debug_snapshot!(kept.by_sql, @"{1, 2, 4, 5, 7, 8}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_lower_ascii_equals() {
    let client = connect(indoc! {r"
        CREATE TEMP TABLE where_rows (id int8, geom geometry(Geometry, 4326), s text);
        INSERT INTO where_rows VALUES
          (1, 'SRID=4326;POINT(0 0)', 'abc'),
          (2, 'SRID=4326;LINESTRING(0 0, 1 1)', 'a%_c'),
          (3, 'SRID=4326;POLYGON((0 0, 1 0, 1 1, 0 0))', NULL),
          (4, 'SRID=4326;MULTIPOINT(0 0, 1 1)', 'ABC'),
          (5, 'SRID=4326;POINT(1 1)', 'Übel'),
          (6, 'SRID=4326;MULTIPOLYGON(((0 0, 1 0, 1 1, 0 0)))', ''),
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', 'a\b'),
          (8, 'SRID=4326;POINT(2 2)', 'abd');
    "})
    .await;
    let kept = kept(
        &client,
        indoc! {r#"
        schema: pg_temp
        table: where_rows
        srid: 4326
        geometry_column: geom
        geometry_type: GEOMETRY
        id_column: id
        properties:
          s: text
        layers:
          l:
            where: "s.lowerAscii() == 'abc'"
        "#},
    )
    .await;
    assert_eq!(kept.by_sql, kept.by_plan);
    insta::assert_compact_debug_snapshot!(kept.by_sql, @"{1, 4}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_upper_ascii_equals() {
    let client = connect(indoc! {r"
        CREATE TEMP TABLE where_rows (id int8, geom geometry(Geometry, 4326), s text);
        INSERT INTO where_rows VALUES
          (1, 'SRID=4326;POINT(0 0)', 'abc'),
          (2, 'SRID=4326;LINESTRING(0 0, 1 1)', 'a%_c'),
          (3, 'SRID=4326;POLYGON((0 0, 1 0, 1 1, 0 0))', NULL),
          (4, 'SRID=4326;MULTIPOINT(0 0, 1 1)', 'ABC'),
          (5, 'SRID=4326;POINT(1 1)', 'Übel'),
          (6, 'SRID=4326;MULTIPOLYGON(((0 0, 1 0, 1 1, 0 0)))', ''),
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', 'a\b'),
          (8, 'SRID=4326;POINT(2 2)', 'abd');
    "})
    .await;
    let kept = kept(
        &client,
        indoc! {r#"
        schema: pg_temp
        table: where_rows
        srid: 4326
        geometry_column: geom
        geometry_type: GEOMETRY
        id_column: id
        properties:
          s: text
        layers:
          l:
            where: "s.upperAscii() == 'ÜBEL'"
        "#},
    )
    .await;
    assert_eq!(kept.by_sql, kept.by_plan);
    insta::assert_compact_debug_snapshot!(kept.by_sql, @"{5}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_has_field() {
    let client = connect(indoc! {r"
        CREATE TEMP TABLE where_rows (id int8, geom geometry(Geometry, 4326), s text);
        INSERT INTO where_rows VALUES
          (1, 'SRID=4326;POINT(0 0)', 'abc'),
          (2, 'SRID=4326;LINESTRING(0 0, 1 1)', 'a%_c'),
          (3, 'SRID=4326;POLYGON((0 0, 1 0, 1 1, 0 0))', NULL),
          (4, 'SRID=4326;MULTIPOINT(0 0, 1 1)', 'ABC'),
          (5, 'SRID=4326;POINT(1 1)', 'Übel'),
          (6, 'SRID=4326;MULTIPOLYGON(((0 0, 1 0, 1 1, 0 0)))', ''),
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', 'a\b'),
          (8, 'SRID=4326;POINT(2 2)', 'abd');
    "})
    .await;
    let kept = kept(
        &client,
        indoc! {r"
        schema: pg_temp
        table: where_rows
        srid: 4326
        geometry_column: geom
        geometry_type: GEOMETRY
        id_column: id
        properties:
          s: text
        layers:
          l:
            where: 'has(feature.s)'
        "},
    )
    .await;
    assert_eq!(kept.by_sql, kept.by_plan);
    insta::assert_compact_debug_snapshot!(kept.by_sql, @"{1, 2, 4, 5, 6, 7, 8}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_negated_has_field() {
    let client = connect(indoc! {r"
        CREATE TEMP TABLE where_rows (id int8, geom geometry(Geometry, 4326), s text);
        INSERT INTO where_rows VALUES
          (1, 'SRID=4326;POINT(0 0)', 'abc'),
          (2, 'SRID=4326;LINESTRING(0 0, 1 1)', 'a%_c'),
          (3, 'SRID=4326;POLYGON((0 0, 1 0, 1 1, 0 0))', NULL),
          (4, 'SRID=4326;MULTIPOINT(0 0, 1 1)', 'ABC'),
          (5, 'SRID=4326;POINT(1 1)', 'Übel'),
          (6, 'SRID=4326;MULTIPOLYGON(((0 0, 1 0, 1 1, 0 0)))', ''),
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', 'a\b'),
          (8, 'SRID=4326;POINT(2 2)', 'abd');
    "})
    .await;
    let kept = kept(
        &client,
        indoc! {r"
        schema: pg_temp
        table: where_rows
        srid: 4326
        geometry_column: geom
        geometry_type: GEOMETRY
        id_column: id
        properties:
          s: text
        layers:
          l:
            where: '!has(feature.s)'
        "},
    )
    .await;
    assert_eq!(kept.by_sql, kept.by_plan);
    insta::assert_compact_debug_snapshot!(kept.by_sql, @"{3}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_key_in_feature() {
    let client = connect(indoc! {r"
        CREATE TEMP TABLE where_rows (id int8, geom geometry(Geometry, 4326), d date);
        INSERT INTO where_rows VALUES
          (1, 'SRID=4326;POINT(0 0)', '2020-01-01'),
          (2, 'SRID=4326;LINESTRING(0 0, 1 1)', NULL),
          (3, 'SRID=4326;POLYGON((0 0, 1 0, 1 1, 0 0))', NULL),
          (4, 'SRID=4326;MULTIPOINT(0 0, 1 1)', '1999-12-31'),
          (5, 'SRID=4326;POINT(1 1)', NULL),
          (6, 'SRID=4326;MULTIPOLYGON(((0 0, 1 0, 1 1, 0 0)))', '2020-01-01'),
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', NULL),
          (8, 'SRID=4326;POINT(2 2)', NULL);
    "})
    .await;
    let kept = kept(
        &client,
        indoc! {r#"
        schema: pg_temp
        table: where_rows
        srid: 4326
        geometry_column: geom
        geometry_type: GEOMETRY
        id_column: id
        properties:
          d: date
        layers:
          l:
            where: "'d' in feature"
        "#},
    )
    .await;
    assert_eq!(kept.by_sql, kept.by_plan);
    insta::assert_compact_debug_snapshot!(kept.by_sql, @"{1, 4, 6}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_feature_index_equals() {
    let client = connect(indoc! {r"
        CREATE TEMP TABLE where_rows (id int8, geom geometry(Geometry, 4326), s text);
        INSERT INTO where_rows VALUES
          (1, 'SRID=4326;POINT(0 0)', 'abc'),
          (2, 'SRID=4326;LINESTRING(0 0, 1 1)', 'a%_c'),
          (3, 'SRID=4326;POLYGON((0 0, 1 0, 1 1, 0 0))', NULL),
          (4, 'SRID=4326;MULTIPOINT(0 0, 1 1)', 'ABC'),
          (5, 'SRID=4326;POINT(1 1)', 'Übel'),
          (6, 'SRID=4326;MULTIPOLYGON(((0 0, 1 0, 1 1, 0 0)))', ''),
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', 'a\b'),
          (8, 'SRID=4326;POINT(2 2)', 'abd');
    "})
    .await;
    let kept = kept(
        &client,
        indoc! {r#"
        schema: pg_temp
        table: where_rows
        srid: 4326
        geometry_column: geom
        geometry_type: GEOMETRY
        id_column: id
        properties:
          s: text
        layers:
          l:
            where: "feature['s'] == 'abc'"
        "#},
    )
    .await;
    assert_eq!(kept.by_sql, kept.by_plan);
    insta::assert_compact_debug_snapshot!(kept.by_sql, @"{1}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_feature_field_equals() {
    let client = connect(indoc! {r"
        CREATE TEMP TABLE where_rows (id int8, geom geometry(Geometry, 4326), s text);
        INSERT INTO where_rows VALUES
          (1, 'SRID=4326;POINT(0 0)', 'abc'),
          (2, 'SRID=4326;LINESTRING(0 0, 1 1)', 'a%_c'),
          (3, 'SRID=4326;POLYGON((0 0, 1 0, 1 1, 0 0))', NULL),
          (4, 'SRID=4326;MULTIPOINT(0 0, 1 1)', 'ABC'),
          (5, 'SRID=4326;POINT(1 1)', 'Übel'),
          (6, 'SRID=4326;MULTIPOLYGON(((0 0, 1 0, 1 1, 0 0)))', ''),
          (7, 'SRID=4326;MULTILINESTRING((0 0, 1 1))', 'a\b'),
          (8, 'SRID=4326;POINT(2 2)', 'abd');
    "})
    .await;
    let kept = kept(
        &client,
        indoc! {r#"
        schema: pg_temp
        table: where_rows
        srid: 4326
        geometry_column: geom
        geometry_type: GEOMETRY
        id_column: id
        properties:
          s: text
        layers:
          l:
            where: "feature.s == 'abc'"
        "#},
    )
    .await;
    assert_eq!(kept.by_sql, kept.by_plan);
    insta::assert_compact_debug_snapshot!(kept.by_sql, @"{1}");
}
