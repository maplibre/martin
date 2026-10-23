#![cfg(all(feature = "test-pg", feature = "unstable-generate"))]
#![expect(clippy::unwrap_used)]

use std::io::Write as _;

use indoc::indoc;
use martin::generate::spec::file::yaml;
use martin::generate::spec::postgres::Postgres;
use martin::generate::spec::report::Reporter;
use martin::generate::spec::review;
use martin::generate::spec::rows::RowSearch;
use martin_core::tiles::postgres::{PostgresPool, RetryTimeout};

/// Runs `sql` on the test database.
async fn seed(sql: &str) {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL");
    let pool = PostgresPool::new(&url, None, None, None, 1, RetryTimeout::default())
        .await
        .unwrap();
    pool.get().await.unwrap().batch_execute(sql).await.unwrap();
}

/// The `PostgreSQL` backend of the configuration `yaml`.
async fn load(yaml: &str) -> Postgres {
    let mut file = tempfile::NamedTempFile::with_suffix(".yaml").unwrap();
    file.write_all(yaml.as_bytes()).unwrap();
    Postgres::load(file.path()).await.unwrap()
}

#[tokio::test]
async fn postgres_selects_a_row_on_each_path_of_a_where() {
    seed(indoc! {"
        DROP SCHEMA IF EXISTS martin_test_where CASCADE;
        CREATE SCHEMA martin_test_where;
        CREATE TABLE martin_test_where.places (id int8 PRIMARY KEY, population int8, geom geometry(Point, 4326));
        INSERT INTO martin_test_where.places VALUES
          (1, 5000, 'SRID=4326;POINT(16.37 48.20)'),
          (2, 10, 'SRID=4326;POINT(16.38 48.21)'),
          (3, NULL, 'SRID=4326;POINT(16.39 48.22)');
    "})
    .await;
    let backend = load(indoc! {"
        postgres:
          connection_string: ${DATABASE_URL}
          auto_publish: false
          tables:
            places:
              schema: martin_test_where
              table: places
              srid: 4326
              geometry_column: geom
              geometry_type: POINT
              id_column: id
              properties:
                id: int8
                population: int8
              layers:
                places:
                  where: population > 1000
    "})
    .await;
    let review = review(&backend, 14, None, 100, &Reporter::new()).await.unwrap();
    let found: Vec<(String, RowSearch)> = review
        .found
        .iter()
        .map(|found| (found.branch.to_string(), found.row.clone()))
        .collect();
    insta::assert_debug_snapshot!(found, @r#"
    [
        (
            "places: where `population > 1000`",
            Selected {
                rows: "\"population\" > 1000",
                key: 1,
            },
        ),
        (
            "places: not where `population > 1000`",
            Selected {
                rows: "\"population\" <= 1000",
                key: 2,
            },
        ),
        (
            "places: where `population > 1000` with `population` null",
            Selected {
                rows: "\"population\" IS NULL",
                key: 3,
            },
        ),
    ]
    "#);
}

#[tokio::test]
async fn postgres_selects_no_row_for_a_path_no_row_takes() {
    seed(indoc! {"
        DROP SCHEMA IF EXISTS martin_test_none CASCADE;
        CREATE SCHEMA martin_test_none;
        CREATE TABLE martin_test_none.places (id int8 PRIMARY KEY, population int8, geom geometry(Point, 4326));
        INSERT INTO martin_test_none.places VALUES
          (1, 5000, 'SRID=4326;POINT(16.37 48.20)');
    "})
    .await;
    let backend = load(indoc! {"
        postgres:
          connection_string: ${DATABASE_URL}
          auto_publish: false
          tables:
            places:
              schema: martin_test_none
              table: places
              srid: 4326
              geometry_column: geom
              geometry_type: POINT
              id_column: id
              properties:
                id: int8
                population: int8
              layers:
                places:
                  where: population > 1000
    "})
    .await;
    let review = review(&backend, 14, None, 100, &Reporter::new()).await.unwrap();
    let found: Vec<(String, RowSearch)> = review
        .found
        .iter()
        .map(|found| (found.branch.to_string(), found.row.clone()))
        .collect();
    insta::assert_debug_snapshot!(found, @r#"
    [
        (
            "places: where `population > 1000`",
            Selected {
                rows: "\"population\" > 1000",
                key: 1,
            },
        ),
        (
            "places: not where `population > 1000`",
            NoneSelected {
                rows: "\"population\" <= 1000",
            },
        ),
        (
            "places: where `population > 1000` with `population` null",
            NoneSelected {
                rows: "\"population\" IS NULL",
            },
        ),
    ]
    "#);
}

#[tokio::test]
async fn postgres_sweeps_a_path_sql_cannot_express() {
    seed(indoc! {"
        DROP SCHEMA IF EXISTS martin_test_sweep CASCADE;
        CREATE SCHEMA martin_test_sweep;
        CREATE TABLE martin_test_sweep.places (id int8 PRIMARY KEY, name text, geom geometry(Point, 4326));
        INSERT INTO martin_test_sweep.places VALUES
          (1, 'A', 'SRID=4326;POINT(16.37 48.20)'),
          (2, 'B', 'SRID=4326;POINT(16.38 48.21)');
    "})
    .await;
    let backend = load(indoc! {"
        postgres:
          connection_string: ${DATABASE_URL}
          auto_publish: false
          tables:
            places:
              schema: martin_test_sweep
              table: places
              srid: 4326
              geometry_column: geom
              geometry_type: POINT
              id_column: id
              properties:
                id: int8
                name: text
              layers:
                places:
                  attributes:
                    short: \"name + '!' == 'A!' ? 'A' : name\"
    "})
    .await;
    let review = review(&backend, 14, None, 100, &Reporter::new()).await.unwrap();
    let found: Vec<(String, RowSearch)> = review
        .found
        .iter()
        .map(|found| (found.branch.to_string(), found.row.clone()))
        .collect();
    insta::assert_debug_snapshot!(found, @r#"
    [
        (
            "places: every feature",
            Selected {
                rows: "TRUE",
                key: 1,
            },
        ),
        (
            "places: `name + '!' == 'A!' ? 'A' : name`: `(name + '!') == 'A!'` true",
            Swept {
                key: 1,
                because: [
                    CannotSelect(
                        Function(
                            "+",
                        ),
                    ),
                ],
            },
        ),
        (
            "places: `name + '!' == 'A!' ? 'A' : name`: `(name + '!') == 'A!'` false",
            Swept {
                key: 2,
                because: [
                    CannotSelect(
                        Function(
                            "+",
                        ),
                    ),
                ],
            },
        ),
        (
            "places: `name + '!' == 'A!' ? 'A' : name`: `(name + '!') == 'A!'` fails with `name` null",
            NoneSelected {
                rows: "\"name\" IS NULL",
            },
        ),
    ]
    "#);
}

#[tokio::test]
async fn postgres_selects_lines_by_their_shape_at_the_max_zoom() {
    seed(indoc! {"
        DROP SCHEMA IF EXISTS martin_test_shapes CASCADE;
        CREATE SCHEMA martin_test_shapes;
        CREATE TABLE martin_test_shapes.roads (id int8 PRIMARY KEY, geom geometry(LineString, 4326));
        INSERT INTO martin_test_shapes.roads VALUES
          (1, 'SRID=4326;LINESTRING(16.37 48.20, 16.40 48.22)'),
          (2, 'SRID=4326;LINESTRING(16.371 48.201, 16.371002 48.201)'),
          (3, 'SRID=4326;LINESTRING(16.3712 48.2012, 16.3714 48.2012)');
    "})
    .await;
    let backend = load(indoc! {"
        postgres:
          connection_string: ${DATABASE_URL}
          auto_publish: false
          tables:
            roads:
              schema: martin_test_shapes
              table: roads
              srid: 4326
              geometry_column: geom
              geometry_type: LINESTRING
              id_column: id
              properties:
                id: int8
              layers:
                roads:
                  min_size: 0
    "})
    .await;
    let review = review(&backend, 14, None, 100, &Reporter::new()).await.unwrap();
    let found: Vec<(String, RowSearch)> = review
        .found
        .iter()
        .map(|found| (found.branch.to_string(), found.row.clone()))
        .collect();
    insta::assert_debug_snapshot!(found, @r#"
    [
        (
            "roads: every feature",
            Selected {
                rows: "TRUE",
                key: 1,
            },
        ),
        (
            "roads: shorter than one tile unit at z14",
            Selected {
                rows: "(ST_GeometryType(\"geom\"::geometry) IN ('ST_LineString', 'ST_MultiLineString') OR ST_GeometryType(\"geom\"::geometry) IN ('ST_Polygon', 'ST_MultiPolygon', 'ST_Triangle')) AND numrange(0, 0.5971642834779397) @> greatest(ST_XMax(ST_Transform(\"geom\"::geometry, 3857)::box2d) - ST_XMin(ST_Transform(\"geom\"::geometry, 3857)::box2d), ST_YMax(ST_Transform(\"geom\"::geometry, 3857)::box2d) - ST_YMin(ST_Transform(\"geom\"::geometry, 3857)::box2d))::numeric",
                key: 2,
            },
        ),
        (
            "roads: crosses a z14 tile edge",
            Selected {
                rows: "(ST_GeometryType(\"geom\"::geometry) IN ('ST_LineString', 'ST_MultiLineString') OR ST_GeometryType(\"geom\"::geometry) IN ('ST_Polygon', 'ST_MultiPolygon', 'ST_Triangle')) AND (floor((ST_XMin(ST_Transform(\"geom\"::geometry, 3857)::box2d) + 20037508.34278925) / 2445.984905125641) <> floor((ST_XMax(ST_Transform(\"geom\"::geometry, 3857)::box2d) + 20037508.34278925) / 2445.984905125641) OR floor((20037508.34278925 - ST_YMin(ST_Transform(\"geom\"::geometry, 3857)::box2d)) / 2445.984905125641) <> floor((20037508.34278925 - ST_YMax(ST_Transform(\"geom\"::geometry, 3857)::box2d)) / 2445.984905125641))",
                key: 1,
            },
        ),
    ]
    "#);
}

#[tokio::test]
async fn postgres_selects_a_line_that_ends_on_a_horizontal_tile_edge_only_once_it_crosses() {
    seed(indoc! {"
        DROP SCHEMA IF EXISTS martin_test_edges CASCADE;
        CREATE SCHEMA martin_test_edges;
        CREATE TABLE martin_test_edges.roads (id int8 PRIMARY KEY, geom geometry(LineString, 3857));
        INSERT INTO martin_test_edges.roads VALUES
          (1, 'SRID=3857;LINESTRING(100 -100, 200 0)'),
          (2, 'SRID=3857;LINESTRING(100 100, 200 0)');
    "})
    .await;
    let backend = load(indoc! {"
        postgres:
          connection_string: ${DATABASE_URL}
          auto_publish: false
          tables:
            roads:
              schema: martin_test_edges
              table: roads
              srid: 3857
              geometry_column: geom
              geometry_type: LINESTRING
              id_column: id
              properties:
                id: int8
              layers:
                roads:
                  min_size: 0
    "})
    .await;
    let review = review(&backend, 14, None, 100, &Reporter::new()).await.unwrap();
    let found: Vec<(String, RowSearch)> = review
        .found
        .iter()
        .filter(|found| found.branch.to_string().contains("tile edge"))
        .map(|found| (found.branch.to_string(), found.row.clone()))
        .collect();
    insta::assert_debug_snapshot!(found, @r#"
    [
        (
            "roads: crosses a z14 tile edge",
            Selected {
                rows: "(ST_GeometryType(\"geom\"::geometry) IN ('ST_LineString', 'ST_MultiLineString') OR ST_GeometryType(\"geom\"::geometry) IN ('ST_Polygon', 'ST_MultiPolygon', 'ST_Triangle')) AND (floor((ST_XMin(ST_Transform(\"geom\"::geometry, 3857)::box2d) + 20037508.34278925) / 2445.984905125641) <> floor((ST_XMax(ST_Transform(\"geom\"::geometry, 3857)::box2d) + 20037508.34278925) / 2445.984905125641) OR floor((20037508.34278925 - ST_YMin(ST_Transform(\"geom\"::geometry, 3857)::box2d)) / 2445.984905125641) <> floor((20037508.34278925 - ST_YMax(ST_Transform(\"geom\"::geometry, 3857)::box2d)) / 2445.984905125641))",
                key: 2,
            },
        ),
    ]
    "#);
}

#[tokio::test]
async fn postgres_selects_no_row_whose_id_is_null_or_negative() {
    seed(indoc! {"
        DROP SCHEMA IF EXISTS martin_test_keys CASCADE;
        CREATE SCHEMA martin_test_keys;
        CREATE TABLE martin_test_keys.places (id int8, geom geometry(Point, 4326));
        INSERT INTO martin_test_keys.places VALUES
          (NULL, 'SRID=4326;POINT(16.37 48.20)'),
          (-5, 'SRID=4326;POINT(16.38 48.21)'),
          (7, 'SRID=4326;POINT(16.39 48.22)');
    "})
    .await;
    let backend = load(indoc! {"
        postgres:
          connection_string: ${DATABASE_URL}
          auto_publish: false
          tables:
            places:
              schema: martin_test_keys
              table: places
              srid: 4326
              geometry_column: geom
              geometry_type: POINT
              id_column: id
              properties:
                id: int8
              layers:
                places: {}
    "})
    .await;
    let review = review(&backend, 14, None, 100, &Reporter::new()).await.unwrap();
    let found: Vec<(String, RowSearch)> = review
        .found
        .iter()
        .map(|found| (found.branch.to_string(), found.row.clone()))
        .collect();
    insta::assert_debug_snapshot!(found, @r#"
    [
        (
            "places: every feature",
            Selected {
                rows: "TRUE",
                key: 7,
            },
        ),
    ]
    "#);
}

#[tokio::test]
async fn generate_over_postgres_reads_a_row_by_an_id_column_with_a_dash() {
    seed(indoc! {r#"
        DROP SCHEMA IF EXISTS martin_test_quoted CASCADE;
        CREATE SCHEMA martin_test_quoted;
        CREATE TABLE martin_test_quoted.places ("row-id" int8 PRIMARY KEY, geom geometry(Point, 4326));
        INSERT INTO martin_test_quoted.places VALUES
          (1, 'SRID=4326;POINT(16.37 48.20)');
    "#})
    .await;
    let backend = load(indoc! {"
        postgres:
          connection_string: ${DATABASE_URL}
          auto_publish: false
          tables:
            places:
              schema: martin_test_quoted
              table: places
              srid: 4326
              geometry_column: geom
              geometry_type: POINT
              id_column: row-id
              properties:
                row-id: int8
              layers:
                places: {}
    "})
    .await;
    let mut review = review(&backend, 14, None, 100, &Reporter::new()).await.unwrap();
    review.accept_all();
    insta::assert_snapshot!(yaml(&review.spec()), @r#"
    max_zoom: 14
    cases:
    - source: places
      paths:
      - "places: every feature"
      input:
        geometry: POINT(16.37 48.2)
        row-id: 1
      output:
      - layer: places
        id: 1
        geometry: point
        zooms: 0-14
        attributes:
          row-id: 1
    "#);
}

#[tokio::test]
async fn generate_over_postgres_writes_each_row_as_bulk_mode_reads_it() {
    seed(indoc! {"
        DROP SCHEMA IF EXISTS martin_test_generate CASCADE;
        CREATE SCHEMA martin_test_generate;
        CREATE TABLE martin_test_generate.places (id int8 PRIMARY KEY, name text, population int8, geom geometry(Point, 4326));
        INSERT INTO martin_test_generate.places VALUES
          (1, 'Wien', 2000000, 'SRID=4326;POINT(16.37 48.20)'),
          (2, 'Baden', 26000, 'SRID=4326;POINT(16.23 48.01)');
    "})
    .await;
    let backend = load(indoc! {"
        postgres:
          connection_string: ${DATABASE_URL}
          auto_publish: false
          tables:
            places:
              schema: martin_test_generate
              table: places
              srid: 4326
              geometry_column: geom
              geometry_type: POINT
              id_column: id
              properties:
                id: int8
                name: text
                population: int8
              layers:
                places:
                  minzoom: \"population > 1000000 ? 2 : 8\"
                  attributes:
                    name: name
    "})
    .await;
    let mut review = review(&backend, 14, None, 100, &Reporter::new()).await.unwrap();
    review.accept_all();
    insta::assert_snapshot!(yaml(&review.spec()), @r#"
    max_zoom: 14
    cases:
    - source: places
      paths:
      - "places: every feature"
      - "places: `population > 1000000 ? 2 : 8`: `population > 1000000` true"
      input:
        geometry: POINT(16.37 48.2)
        id: 1
        name: Wien
        population: 2000000
      output:
      - layer: places
        id: 1
        geometry: point
        zooms: 2-14
        attributes:
          name: Wien
    - source: places
      paths:
      - "places: `population > 1000000 ? 2 : 8`: `population > 1000000` false"
      - "places: every feature"
      input:
        geometry: POINT(16.23 48.01)
        id: 2
        name: Baden
        population: 26000
      output:
      - layer: places
        id: 2
        geometry: point
        zooms: 8-14
        attributes:
          name: Baden
    "#);
}
