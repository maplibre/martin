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
use martin_tilegen::plan::{Plan, RowFilter};

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
        let geom = feature.geometry.feature_geom().expect("undecoded geometry");
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
            geometry: point
        "},
    )
    .await;
    assert_eq!(in_sql, in_plan);
    insta::assert_compact_debug_snapshot!(in_sql, @"{1, 2}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_line_geometry() {
    let client = connect(indoc! {r"
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
    insta::assert_compact_debug_snapshot!(in_sql, @"{3, 4, 7, 9}");
}

#[tokio::test]
async fn sql_matches_the_plan_on_polygon_geometry() {
    let client = connect(indoc! {r"
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
            geometry: polygon
        "},
    )
    .await;
    assert_eq!(in_sql, in_plan);
    insta::assert_compact_debug_snapshot!(in_sql, @"{5, 6, 8}");
}
