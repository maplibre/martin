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

/// The database the tests seed, apart from the one `DATABASE_URL` names, whose sources other tests discover.
const SEEDS: &str = "martin_test_seeds";

/// A connection pool on the database `url` names.
async fn pool(url: &str) -> PostgresPool {
    PostgresPool::new(url, None, None, None, 1, RetryTimeout::default())
        .await
        .unwrap()
}

/// `DATABASE_URL` naming [`SEEDS`] instead of its database, which this creates with `PostGIS` on first use.
async fn seeds_url() -> String {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL");
    let (base, params) = url
        .split_once('?')
        .map_or((url.as_str(), ""), |(base, params)| (base, params));
    let (server, _) = base.rsplit_once('/').expect("DATABASE_URL names a database");
    let seeds = match params {
        "" => format!("{server}/{SEEDS}"),
        params => format!("{server}/{SEEDS}?{params}"),
    };
    let main = pool(&url).await;
    let conn = main.get().await.unwrap();
    conn.batch_execute("SELECT pg_advisory_lock(hashtext('martin_test_seeds'))")
        .await
        .unwrap();
    let exists = conn
        .query_opt("SELECT 1 FROM pg_database WHERE datname = $1", &[&SEEDS])
        .await
        .unwrap();
    if exists.is_none() {
        conn.batch_execute(&format!("CREATE DATABASE {SEEDS}")).await.unwrap();
    }
    pool(&seeds)
        .await
        .get()
        .await
        .unwrap()
        .batch_execute("CREATE EXTENSION IF NOT EXISTS postgis")
        .await
        .unwrap();
    conn.batch_execute("SELECT pg_advisory_unlock(hashtext('martin_test_seeds'))")
        .await
        .unwrap();
    seeds
}

/// Runs `sql` on the database the tests seed.
async fn seed(sql: &str) {
    pool(&seeds_url().await)
        .await
        .get()
        .await
        .unwrap()
        .batch_execute(sql)
        .await
        .unwrap();
}

/// The `PostgreSQL` backend of the configuration `yaml`, its `${DATABASE_URL}` the database the tests seed.
async fn load(yaml: &str) -> Postgres {
    let yaml = yaml.replace("${DATABASE_URL}", &seeds_url().await);
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
            "places: kept by `where: population > 1000`",
            Selected {
                rows: "\"population\" > 1000",
                key: 1,
            },
        ),
        (
            "places: dropped, `where: population > 1000` is false",
            Selected {
                rows: "\"population\" <= 1000",
                key: 2,
            },
        ),
        (
            "places: dropped, `population` is null in `where: population > 1000`",
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
            "places: kept by `where: population > 1000`",
            Selected {
                rows: "\"population\" > 1000",
                key: 1,
            },
        ),
        (
            "places: dropped, `where: population > 1000` is false",
            NoneSelected {
                rows: "\"population\" <= 1000",
            },
        ),
        (
            "places: dropped, `population` is null in `where: population > 1000`",
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
            "places: attributes.short 'A', `(name + '!') == 'A!'` is true",
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
            "places: attributes.short `name`, `(name + '!') == 'A!'` is false",
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
            "places: attributes.short, `name` is null in `(name + '!') == 'A!'`",
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
                rows: "(ST_GeometryType(\"geom\"::geometry) IN ('ST_LineString', 'ST_MultiLineString') OR ST_GeometryType(\"geom\"::geometry) IN ('ST_Polygon', 'ST_MultiPolygon', 'ST_Triangle')) AND numrange(0, 0.5971642834779397) @> greatest(ST_XMax(ST_Transform(\"geom\"::geometry, 3857)::box2d) - ST_XMin(ST_Transform(\"geom\"::geometry, 3857)::box2d), least(greatest(ST_YMax(ST_Transform(\"geom\"::geometry, 3857)::box2d), -20037508.34278925), 20037508.34278925) - least(greatest(ST_YMin(ST_Transform(\"geom\"::geometry, 3857)::box2d), -20037508.34278925), 20037508.34278925))::numeric",
                key: 2,
            },
        ),
        (
            "roads: crosses a z14 tile edge",
            Selected {
                rows: "(ST_GeometryType(\"geom\"::geometry) IN ('ST_LineString', 'ST_MultiLineString') OR ST_GeometryType(\"geom\"::geometry) IN ('ST_Polygon', 'ST_MultiPolygon', 'ST_Triangle')) AND (floor((ST_XMin(ST_Transform(\"geom\"::geometry, 3857)::box2d) + 20037508.34278925) / 2445.984905125641) <> floor((ST_XMax(ST_Transform(\"geom\"::geometry, 3857)::box2d) + 20037508.34278925) / 2445.984905125641) OR floor((20037508.34278925 - least(greatest(ST_YMin(ST_Transform(\"geom\"::geometry, 3857)::box2d), -20037508.34278925), 20037508.34278925)) / 2445.984905125641) <> floor((20037508.34278925 - least(greatest(ST_YMax(ST_Transform(\"geom\"::geometry, 3857)::box2d), -20037508.34278925), 20037508.34278925)) / 2445.984905125641))",
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
                rows: "(ST_GeometryType(\"geom\"::geometry) IN ('ST_LineString', 'ST_MultiLineString') OR ST_GeometryType(\"geom\"::geometry) IN ('ST_Polygon', 'ST_MultiPolygon', 'ST_Triangle')) AND (floor((ST_XMin(ST_Transform(\"geom\"::geometry, 3857)::box2d) + 20037508.34278925) / 2445.984905125641) <> floor((ST_XMax(ST_Transform(\"geom\"::geometry, 3857)::box2d) + 20037508.34278925) / 2445.984905125641) OR floor((20037508.34278925 - least(greatest(ST_YMin(ST_Transform(\"geom\"::geometry, 3857)::box2d), -20037508.34278925), 20037508.34278925)) / 2445.984905125641) <> floor((20037508.34278925 - least(greatest(ST_YMax(ST_Transform(\"geom\"::geometry, 3857)::box2d), -20037508.34278925), 20037508.34278925)) / 2445.984905125641))",
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
async fn postgres_gives_a_not_null_column_no_null_path() {
    seed(indoc! {"
        DROP SCHEMA IF EXISTS martin_test_not_null CASCADE;
        CREATE SCHEMA martin_test_not_null;
        CREATE TABLE martin_test_not_null.places (id int8 PRIMARY KEY, kind text NOT NULL, geom geometry(Point, 4326));
        INSERT INTO martin_test_not_null.places VALUES
          (1, 'major', 'SRID=4326;POINT(16.37 48.20)'),
          (2, 'minor', 'SRID=4326;POINT(16.38 48.21)');
    "})
    .await;
    let backend = load(indoc! {"
        postgres:
          connection_string: ${DATABASE_URL}
          auto_publish: false
          tables:
            places:
              schema: martin_test_not_null
              table: places
              srid: 4326
              geometry_column: geom
              geometry_type: POINT
              id_column: id
              properties:
                id: int8
                kind: text
              layers:
                places:
                  minzoom: \"kind == 'major' ? 5 : 8\"
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
            "places: minzoom 5, `kind == 'major'` is true",
            Selected {
                rows: "\"kind\" = 'major'",
                key: 1,
            },
        ),
        (
            "places: minzoom 8, `kind == 'major'` is false",
            Selected {
                rows: "\"kind\" IS DISTINCT FROM 'major'",
                key: 2,
            },
        ),
    ]
    "#);
}

#[tokio::test]
async fn postgres_gives_a_path_to_each_value_a_check_allows_and_to_null() {
    seed(indoc! {"
        DROP SCHEMA IF EXISTS martin_test_check CASCADE;
        CREATE SCHEMA martin_test_check;
        CREATE TABLE martin_test_check.places (
          id int8 PRIMARY KEY,
          kind text CHECK (kind IN ('major', 'minor')),
          geom geometry(Point, 4326)
        );
        INSERT INTO martin_test_check.places VALUES
          (1, 'major', 'SRID=4326;POINT(16.37 48.20)'),
          (2, 'minor', 'SRID=4326;POINT(16.38 48.21)');
    "})
    .await;
    let backend = load(indoc! {"
        postgres:
          connection_string: ${DATABASE_URL}
          auto_publish: false
          tables:
            places:
              schema: martin_test_check
              table: places
              srid: 4326
              geometry_column: geom
              geometry_type: POINT
              id_column: id
              properties:
                id: int8
                kind: text
              layers:
                places:
                  minzoom: \"kind == 'other' ? 12 : 5\"
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
            "places: kind 'major' -> minzoom 5",
            Selected {
                rows: "\"kind\" = 'major'",
                key: 1,
            },
        ),
        (
            "places: kind 'minor' -> minzoom 5",
            Selected {
                rows: "\"kind\" = 'minor'",
                key: 2,
            },
        ),
        (
            "places: kind null -> minzoom 5",
            NoneSelected {
                rows: "\"kind\" IS NULL",
            },
        ),
    ]
    "#);
}

#[tokio::test]
async fn postgres_gives_the_id_column_no_null_path_as_no_row_without_an_id_is_selected() {
    seed(indoc! {"
        DROP SCHEMA IF EXISTS martin_test_null_key_path CASCADE;
        CREATE SCHEMA martin_test_null_key_path;
        CREATE TABLE martin_test_null_key_path.places (id int8, geom geometry(Point, 4326));
        INSERT INTO martin_test_null_key_path.places VALUES
          (NULL, 'SRID=4326;POINT(16.37 48.20)'),
          (1, 'SRID=4326;POINT(16.38 48.21)'),
          (2, 'SRID=4326;POINT(16.39 48.22)');
    "})
    .await;
    let backend = load(indoc! {"
        postgres:
          connection_string: ${DATABASE_URL}
          auto_publish: false
          tables:
            places:
              schema: martin_test_null_key_path
              table: places
              srid: 4326
              geometry_column: geom
              geometry_type: POINT
              id_column: id
              properties:
                id: int8
              layers:
                places:
                  minzoom: \"id == 1 ? 5 : 8\"
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
            "places: minzoom 5, `id == 1` is true",
            Selected {
                rows: "\"id\" = 1",
                key: 1,
            },
        ),
        (
            "places: minzoom 8, `id == 1` is false",
            Selected {
                rows: "\"id\" IS DISTINCT FROM 1",
                key: 2,
            },
        ),
    ]
    "#);
}

#[tokio::test]
async fn postgres_gives_a_path_to_each_label_of_an_enum_column() {
    seed(indoc! {"
        DROP SCHEMA IF EXISTS martin_test_enum CASCADE;
        CREATE SCHEMA martin_test_enum;
        CREATE TYPE martin_test_enum.kind AS ENUM ('major', 'minor');
        CREATE TABLE martin_test_enum.places (id int8 PRIMARY KEY, kind martin_test_enum.kind, geom geometry(Point, 4326));
        INSERT INTO martin_test_enum.places VALUES
          (1, 'major', 'SRID=4326;POINT(16.37 48.20)'),
          (2, 'minor', 'SRID=4326;POINT(16.38 48.21)'),
          (3, NULL, 'SRID=4326;POINT(16.39 48.22)');
    "})
    .await;
    let backend = load(indoc! {"
        postgres:
          connection_string: ${DATABASE_URL}
          auto_publish: false
          tables:
            places:
              schema: martin_test_enum
              table: places
              srid: 4326
              geometry_column: geom
              geometry_type: POINT
              id_column: id
              properties:
                id: int8
                kind: text
              layers:
                places:
                  minzoom: \"kind == 'major' ? 5 : 8\"
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
            "places: kind 'major' -> minzoom 5",
            Selected {
                rows: "\"kind\"::text = 'major'",
                key: 1,
            },
        ),
        (
            "places: kind 'minor' -> minzoom 8",
            Selected {
                rows: "\"kind\"::text = 'minor'",
                key: 2,
            },
        ),
        (
            "places: kind null -> minzoom 8",
            Selected {
                rows: "\"kind\"::text IS NULL",
                key: 3,
            },
        ),
    ]
    "#);
}

#[tokio::test]
async fn postgres_gives_a_path_to_each_value_a_check_on_a_varchar_column_allows() {
    seed(indoc! {"
        DROP SCHEMA IF EXISTS martin_test_check_varchar CASCADE;
        CREATE SCHEMA martin_test_check_varchar;
        CREATE TABLE martin_test_check_varchar.places (
          id int8 PRIMARY KEY,
          kind varchar(10) NOT NULL CHECK (kind IN ('major', 'minor')),
          geom geometry(Point, 4326)
        );
        INSERT INTO martin_test_check_varchar.places VALUES
          (1, 'major', 'SRID=4326;POINT(16.37 48.20)'),
          (2, 'minor', 'SRID=4326;POINT(16.38 48.21)');
    "})
    .await;
    let backend = load(indoc! {"
        postgres:
          connection_string: ${DATABASE_URL}
          auto_publish: false
          tables:
            places:
              schema: martin_test_check_varchar
              table: places
              srid: 4326
              geometry_column: geom
              geometry_type: POINT
              id_column: id
              properties:
                id: int8
                kind: varchar
              layers:
                places:
                  minzoom: \"kind == 'major' ? 5 : 8\"
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
            "places: kind 'major' -> minzoom 5",
            Selected {
                rows: "\"kind\" = 'major'",
                key: 1,
            },
        ),
        (
            "places: kind 'minor' -> minzoom 8",
            Selected {
                rows: "\"kind\" = 'minor'",
                key: 2,
            },
        ),
    ]
    "#);
}

#[tokio::test]
async fn postgres_gives_a_path_to_each_integer_a_check_allows() {
    seed(indoc! {"
        DROP SCHEMA IF EXISTS martin_test_check_int CASCADE;
        CREATE SCHEMA martin_test_check_int;
        CREATE TABLE martin_test_check_int.places (
          id int8 PRIMARY KEY,
          kind int4 NOT NULL CHECK (kind IN (-1, 2)),
          geom geometry(Point, 4326)
        );
        INSERT INTO martin_test_check_int.places VALUES
          (1, -1, 'SRID=4326;POINT(16.37 48.20)'),
          (2, 2, 'SRID=4326;POINT(16.38 48.21)');
    "})
    .await;
    let backend = load(indoc! {"
        postgres:
          connection_string: ${DATABASE_URL}
          auto_publish: false
          tables:
            places:
              schema: martin_test_check_int
              table: places
              srid: 4326
              geometry_column: geom
              geometry_type: POINT
              id_column: id
              properties:
                id: int8
                kind: int4
              layers:
                places:
                  minzoom: \"kind < 0 ? 5 : 8\"
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
            "places: kind -1 -> minzoom 5",
            Selected {
                rows: "\"kind\" = -1",
                key: 1,
            },
        ),
        (
            "places: kind 2 -> minzoom 8",
            Selected {
                rows: "\"kind\" = 2",
                key: 2,
            },
        ),
    ]
    "#);
}

#[tokio::test]
async fn postgres_gives_the_one_value_a_check_allows_a_path() {
    seed(indoc! {"
        DROP SCHEMA IF EXISTS martin_test_check_one CASCADE;
        CREATE SCHEMA martin_test_check_one;
        CREATE TABLE martin_test_check_one.places (
          id int8 PRIMARY KEY,
          kind text NOT NULL CHECK (kind = 'major'),
          geom geometry(Point, 4326)
        );
        INSERT INTO martin_test_check_one.places VALUES
          (1, 'major', 'SRID=4326;POINT(16.37 48.20)');
    "})
    .await;
    let backend = load(indoc! {"
        postgres:
          connection_string: ${DATABASE_URL}
          auto_publish: false
          tables:
            places:
              schema: martin_test_check_one
              table: places
              srid: 4326
              geometry_column: geom
              geometry_type: POINT
              id_column: id
              properties:
                id: int8
                kind: text
              layers:
                places:
                  minzoom: \"kind == 'major' ? 5 : 8\"
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
            "places: kind 'major' -> minzoom 5",
            Selected {
                rows: "\"kind\" = 'major'",
                key: 1,
            },
        ),
    ]
    "#);
}

#[tokio::test]
async fn postgres_reads_check_values_with_quotes_and_commas() {
    seed(indoc! {"
        DROP SCHEMA IF EXISTS martin_test_check_quotes CASCADE;
        CREATE SCHEMA martin_test_check_quotes;
        CREATE TABLE martin_test_check_quotes.places (
          id int8 PRIMARY KEY,
          kind text NOT NULL CHECK (kind IN ('it''s', 'a, b')),
          geom geometry(Point, 4326)
        );
        INSERT INTO martin_test_check_quotes.places VALUES
          (1, 'it''s', 'SRID=4326;POINT(16.37 48.20)'),
          (2, 'a, b', 'SRID=4326;POINT(16.38 48.21)');
    "})
    .await;
    let backend = load(indoc! {"
        postgres:
          connection_string: ${DATABASE_URL}
          auto_publish: false
          tables:
            places:
              schema: martin_test_check_quotes
              table: places
              srid: 4326
              geometry_column: geom
              geometry_type: POINT
              id_column: id
              properties:
                id: int8
                kind: text
              layers:
                places:
                  minzoom: \"kind == 'a, b' ? 5 : 8\"
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
            "places: kind 'it\\'s' -> minzoom 8",
            Selected {
                rows: "\"kind\" = 'it''s'",
                key: 1,
            },
        ),
        (
            "places: kind 'a, b' -> minzoom 5",
            Selected {
                rows: "\"kind\" = 'a, b'",
                key: 2,
            },
        ),
    ]
    "#);
}

#[tokio::test]
async fn postgres_keeps_the_choice_paths_of_a_column_whose_check_lists_no_values() {
    seed(indoc! {"
        DROP SCHEMA IF EXISTS martin_test_check_length CASCADE;
        CREATE SCHEMA martin_test_check_length;
        CREATE TABLE martin_test_check_length.places (
          id int8 PRIMARY KEY,
          kind text NOT NULL CHECK (length(kind) < 6),
          geom geometry(Point, 4326)
        );
        INSERT INTO martin_test_check_length.places VALUES
          (1, 'major', 'SRID=4326;POINT(16.37 48.20)'),
          (2, 'minor', 'SRID=4326;POINT(16.38 48.21)');
    "})
    .await;
    let backend = load(indoc! {"
        postgres:
          connection_string: ${DATABASE_URL}
          auto_publish: false
          tables:
            places:
              schema: martin_test_check_length
              table: places
              srid: 4326
              geometry_column: geom
              geometry_type: POINT
              id_column: id
              properties:
                id: int8
                kind: text
              layers:
                places:
                  minzoom: \"kind == 'major' ? 5 : 8\"
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
            "places: minzoom 5, `kind == 'major'` is true",
            Selected {
                rows: "\"kind\" = 'major'",
                key: 1,
            },
        ),
        (
            "places: minzoom 8, `kind == 'major'` is false",
            Selected {
                rows: "\"kind\" IS DISTINCT FROM 'major'",
                key: 2,
            },
        ),
    ]
    "#);
}

#[tokio::test]
async fn postgres_ignores_a_check_that_is_not_validated() {
    seed(indoc! {"
        DROP SCHEMA IF EXISTS martin_test_check_not_valid CASCADE;
        CREATE SCHEMA martin_test_check_not_valid;
        CREATE TABLE martin_test_check_not_valid.places (id int8 PRIMARY KEY, kind text NOT NULL, geom geometry(Point, 4326));
        INSERT INTO martin_test_check_not_valid.places VALUES
          (1, 'major', 'SRID=4326;POINT(16.37 48.20)'),
          (2, 'other', 'SRID=4326;POINT(16.38 48.21)');
        ALTER TABLE martin_test_check_not_valid.places ADD CHECK (kind IN ('major', 'minor')) NOT VALID;
    "})
    .await;
    let backend = load(indoc! {"
        postgres:
          connection_string: ${DATABASE_URL}
          auto_publish: false
          tables:
            places:
              schema: martin_test_check_not_valid
              table: places
              srid: 4326
              geometry_column: geom
              geometry_type: POINT
              id_column: id
              properties:
                id: int8
                kind: text
              layers:
                places:
                  minzoom: \"kind == 'other' ? 12 : 5\"
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
            "places: minzoom 12, `kind == 'other'` is true",
            Selected {
                rows: "\"kind\" = 'other'",
                key: 2,
            },
        ),
        (
            "places: minzoom 5, `kind == 'other'` is false",
            Selected {
                rows: "\"kind\" IS DISTINCT FROM 'other'",
                key: 1,
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
      - "places: minzoom 2, `population > 1000000` is true"
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
      - "places: minzoom 8, `population > 1000000` is false"
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

#[tokio::test]
async fn generate_over_postgres_reads_the_rows_as_they_were_when_it_loaded() {
    seed(indoc! {"
        DROP SCHEMA IF EXISTS martin_test_snapshot CASCADE;
        CREATE SCHEMA martin_test_snapshot;
        CREATE TABLE martin_test_snapshot.places (id int8 PRIMARY KEY, name text, geom geometry(Point, 4326));
        INSERT INTO martin_test_snapshot.places VALUES
          (1, 'Wien', 'SRID=4326;POINT(16.37 48.20)');
    "})
    .await;
    let backend = load(indoc! {"
        postgres:
          connection_string: ${DATABASE_URL}
          auto_publish: false
          tables:
            places:
              schema: martin_test_snapshot
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
                    name: name
    "})
    .await;
    seed("UPDATE martin_test_snapshot.places SET name = 'Vienna' WHERE id = 1;").await;
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
        id: 1
        name: Wien
      output:
      - layer: places
        id: 1
        geometry: point
        zooms: 0-14
        attributes:
          name: Wien
    "#);
}

#[tokio::test]
async fn postgres_needs_a_pool_of_two_connections() {
    let mut file = tempfile::NamedTempFile::with_suffix(".yaml").unwrap();
    file.write_all(
        indoc! {"
            postgres:
              connection_string: ${DATABASE_URL}
              pool_size: 1
              auto_publish: false
              tables:
                places:
                  schema: public
                  table: points1
                  srid: 4326
                  geometry_column: geom
                  geometry_type: POINT
                  id_column: gid
                  properties:
                    gid: int4
                  layers:
                    places: {}
        "}
        .as_bytes(),
    )
    .unwrap();
    let error = Postgres::load(file.path()).await.err().unwrap();
    insta::assert_snapshot!(error, @"`martin test` holds one connection open for its snapshot and reads through another, but `pool_size` is 1; set it to at least 2");
}
