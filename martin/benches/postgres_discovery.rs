use std::time::Duration;

use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use martin::config::file::init_aws_lc_tls;
use martin::config::file::postgres::{PostgresAutoDiscoveryBuilder, PostgresConfig};
use martin_core::config::IdResolver;
use testcontainers_modules::postgres::Postgres;
use testcontainers_modules::testcontainers::ImageExt;
use testcontainers_modules::testcontainers::runners::SyncRunner;

// Benchmark sizes
const SIZES: &[usize] = &[10, 100, 200];

/// Setup [`PostGIS`](https://hub.docker.com/r/postgis/postgis/) container
fn setup_postgres_container() -> (
    testcontainers_modules::testcontainers::Container<Postgres>,
    String,
) {
    let container = Postgres::default()
        .with_name("postgis/postgis")
        .with_tag("18-3.6-alpine")
        .with_env_var("POSTGRES_DB", "bench")
        .with_env_var("POSTGRES_USER", "postgres")
        .with_env_var("POSTGRES_PASSWORD", "postgres")
        .with_env_var("POSTGRES_HOST_AUTH_METHOD", "trust")
        .start()
        .expect("Failed to start container");

    let host = container.get_host().expect("Failed to get host");
    let port = container
        .get_host_port_ipv4(5432)
        .expect("Failed to get port");

    let connection_string =
        format!("postgres://postgres:postgres@{host}:{port}/bench?sslmode=disable");

    (container, connection_string)
}

/// Create test tables with various geometries
async fn populate_tables(connection_string: &str, count: usize) {
    let pool =
        martin_core::tiles::postgres::PostgresPool::new(connection_string, None, None, None, 10)
            .await
            .expect("Failed to create pool");

    let client = pool.get().await.expect("Failed to get client");

    for i in 0..count {
        // Mix geometry types
        let geometry_type = match i % 4 {
            0 => "Point",
            1 => "LineString",
            2 => "Polygon",
            _ => "MultiPolygon",
        };

        // Vary SRIDs
        let srid = match i % 3 {
            0 => 4326,
            1 => 3857,
            _ => 2154, // French projection
        };

        client
            .execute(
                &format!(
                    "CREATE TABLE bench_table_{i} (
                            id SERIAL PRIMARY KEY,
                            geom geometry({geometry_type}, {srid}),
                            name VARCHAR(255),
                            description TEXT,
                            category VARCHAR(100),
                            created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
                            properties JSONB
                        )"
                ),
                &[],
            )
            .await
            .expect("Failed to create table");

        // Add spatial index
        client
            .execute(
                &format!(
                    "CREATE INDEX bench_table_{i}_geom_idx ON bench_table_{i} USING GIST (geom)"
                ),
                &[],
            )
            .await
            .expect("Failed to create spatial index");

        // Some tables get additional indexes
        if i % 2 == 0 {
            client
                .execute(
                    &format!(
                        "CREATE INDEX bench_table_{i}_category_idx ON bench_table_{i} (category)"
                    ),
                    &[],
                )
                .await
                .ok();
        }

        // Insert sample data
        let sample_geom = match geometry_type {
            "Point" => format!("ST_SetSRID(ST_MakePoint(-73.9857, 40.7484), {srid})",),
            "LineString" => {
                format!(
                    "ST_SetSRID(ST_MakeLine(ST_MakePoint(-73.9857, 40.7484), ST_MakePoint(-73.9757, 40.7584)), {srid})",
                )
            }
            _ => format!("ST_SetSRID(ST_MakeEnvelope(-74.0, 40.7, -73.9, 40.8), {srid})"),
        };

        client
                .execute(
                    &format!(
                        "INSERT INTO bench_table_{i} (geom, name, category, properties) VALUES ({sample_geom}, 'Sample {i}', 'category_{category}', '{{}}'::jsonb)",
                        category = i % 5
                    ),
                    &[],
                )
                .await
                .expect("Failed to insert sample data");
    }

    client.execute("ANALYZE", &[]).await.ok();
}

/// Create test MVT functions
async fn populate_functions(connection_string: &str, count: usize) {
    let pool =
        martin_core::tiles::postgres::PostgresPool::new(connection_string, None, None, None, 10)
            .await
            .expect("Failed to create pool");

    let client = pool.get().await.expect("Failed to get client");

    for i in 0..count {
        // Create different function patterns
        let create_sql = match i % 3 {
            0 => {
                // Basic MVT function
                format!(
                    "CREATE FUNCTION bench_func_{i}(z integer, x integer, y integer)
                         RETURNS bytea AS $$
                         DECLARE
                           result bytea;
                         BEGIN
                           SELECT ST_AsMVT(q, 'bench_func_{i}', 4096, 'geom')
                           INTO result
                           FROM (
                             SELECT
                               ST_AsMVTGeom(
                                 ST_Transform(ST_MakePoint(0, 0), 3857),
                                 ST_TileEnvelope(z, x, y),
                                 4096, 64, true
                               ) AS geom,
                               'test' as name
                           ) q;
                           RETURN COALESCE(result, '\\x00'::bytea);
                         END;
                         $$ LANGUAGE plpgsql IMMUTABLE STRICT PARALLEL SAFE"
                )
            }
            1 => {
                // With query params
                format!(
                    "CREATE FUNCTION bench_func_{i}(z integer, x integer, y integer, query_params json)
                         RETURNS bytea AS $$
                         DECLARE
                           result bytea;
                           filter_value text;
                         BEGIN
                           filter_value := COALESCE(query_params->>'filter', '');

                           SELECT ST_AsMVT(q, 'bench_func_{i}', 4096, 'geom')
                           INTO result
                           FROM (
                             SELECT
                               ST_AsMVTGeom(
                                 ST_Transform(ST_MakePoint(0, 0), 3857),
                                 ST_TileEnvelope(z, x, y),
                                 4096, 64, true
                               ) AS geom,
                               filter_value as filter
                           ) q;
                           RETURN COALESCE(result, '\\x00'::bytea);
                         END;
                         $$ LANGUAGE plpgsql IMMUTABLE PARALLEL SAFE"
                )
            }
            _ => {
                // With ETag support
                format!(
                    "CREATE FUNCTION bench_func_{i}(z integer, x integer, y integer)
                         RETURNS TABLE(mvt bytea, hash text) AS $$
                         DECLARE
                           tile_data bytea;
                         BEGIN
                           SELECT ST_AsMVT(q, 'bench_func_{i}', 4096, 'geom')
                           INTO tile_data
                           FROM (
                             SELECT
                               ST_AsMVTGeom(
                                 ST_Transform(ST_MakePoint(0, 0), 3857),
                                 ST_TileEnvelope(z, x, y),
                                 4096, 64, true
                               ) AS geom
                           ) q;

                           mvt := COALESCE(tile_data, '\\x00'::bytea);
                           hash := md5(mvt::text);
                           RETURN NEXT;
                         END;
                         $$ LANGUAGE plpgsql IMMUTABLE STRICT PARALLEL SAFE"
                )
            }
        };

        client
            .execute(&create_sql, &[])
            .await
            .expect("Failed to create function");

        // Add documentation to some functions
        if i.is_multiple_of(3) {
            let params = if i.is_multiple_of(2) {
                "integer, integer, integer"
            } else {
                "integer, integer, integer, json"
            };
            let comment = format!(
                "COMMENT ON FUNCTION bench_func_{i}({params}) IS 'Benchmark test function bench_func_{i} - returns MVT tiles'",
            );
            client.execute(&comment, &[]).await.ok();
        }
    }
}

async fn discover_tables(config: &PostgresConfig) {
    let builder = PostgresAutoDiscoveryBuilder::new(config, IdResolver::default())
        .await
        .expect("Failed to create builder");

    let tables = builder
        .instantiate_tables()
        .await
        .expect("Failed to discover tables");
    black_box(tables);
}

async fn discover_functions(config: &PostgresConfig) {
    let builder = PostgresAutoDiscoveryBuilder::new(config, IdResolver::default())
        .await
        .expect("Failed to create builder");

    let functions = builder
        .instantiate_functions()
        .await
        .expect("Failed to discover functions");
    black_box(functions);
}

fn bench_table_discovery(c: &mut Criterion) {
    init_aws_lc_tls();
    let runtime = tokio::runtime::Runtime::new().unwrap();

    let mut group = c.benchmark_group("table_discovery");
    group.measurement_time(Duration::from_secs(20));
    group.sample_size(20);

    for size in SIZES {
        let (_container, connection_string) = setup_postgres_container();
        runtime.block_on(populate_tables(&connection_string, *size));

        let config = PostgresConfig {
            connection_string: Some(connection_string.clone()),
            ..Default::default()
        };

        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, _| {
            b.to_async(&runtime).iter(|| discover_tables(&config));
        });
    }

    group.finish();
}

fn bench_function_discovery(c: &mut Criterion) {
    init_aws_lc_tls();
    let runtime = tokio::runtime::Runtime::new().unwrap();

    let mut group = c.benchmark_group("function_discovery");
    group.measurement_time(Duration::from_secs(20));
    group.sample_size(20);

    for size in SIZES {
        let (_container, connection_string) = setup_postgres_container();
        runtime.block_on(populate_functions(&connection_string, *size));

        let config = PostgresConfig {
            connection_string: Some(connection_string.clone()),
            ..Default::default()
        };

        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, _| {
            b.to_async(&runtime).iter(|| discover_functions(&config));
        });
    }

    group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default();
    targets = bench_table_discovery, bench_function_discovery
}

criterion_main!(benches);
