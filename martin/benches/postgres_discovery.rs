use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use martin::config::file::postgres::{PostgresAutoDiscoveryBuilder, PostgresConfig};
use martin_core::config::IdResolver;
use martin_core::tiles::postgres::PostgresPool;
use pprof::criterion::{Output, PProfProfiler};

// Different sizes to benchmark
const SIZES: &[usize] = &[10, 100, 500];

/// Initialize rustls crypto provider once
fn init_crypto() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        rustls::crypto::ring::default_provider()
            .install_default()
            .expect("Failed to install rustls crypto provider");
    });
}

/// Get database connection URL from environment or use default test database
fn get_database_url() -> String {
    std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5411/db".to_string())
}

/// Clean up all benchmark tables and functions
async fn cleanup_database() {
    let database_url = get_database_url();
    let pool = PostgresPool::new(&database_url, None, None, None, 10)
        .await
        .expect("Failed to create pool");

    let client = pool.get().await.expect("Failed to get client");

    // Drop all benchmark tables
    let tables_result = client
        .query(
            "SELECT tablename FROM pg_tables WHERE schemaname = 'public' AND tablename LIKE 'bench_%'",
            &[],
        )
        .await
        .expect("Failed to query tables");

    for row in tables_result {
        let table_name: String = row.get(0);
        client
            .execute(&format!("DROP TABLE IF EXISTS {} CASCADE", table_name), &[])
            .await
            .ok();
    }

    // Drop all benchmark functions
    let funcs_result = client
        .query(
            "SELECT proname, pg_get_function_identity_arguments(oid) as args
             FROM pg_proc
             WHERE pronamespace = 'public'::regnamespace AND proname LIKE 'bench_%'",
            &[],
        )
        .await
        .expect("Failed to query functions");

    for row in funcs_result {
        let func_name: String = row.get(0);
        let args: String = row.get(1);
        client
            .execute(
                &format!("DROP FUNCTION IF EXISTS {}({}) CASCADE", func_name, args),
                &[],
            )
            .await
            .ok();
    }
}

/// Setup database with realistic tables
async fn setup_tables(count: usize) -> PostgresConfig {
    let database_url = get_database_url();

    let config = PostgresConfig {
        connection_string: Some(database_url.clone()),
        ..Default::default()
    };

    let pool = PostgresPool::new(&database_url, None, None, None, 10)
        .await
        .expect("Failed to create pool");

    let client = pool.get().await.expect("Failed to get client");

    // Ensure PostGIS extension exists
    client
        .execute("CREATE EXTENSION IF NOT EXISTS postgis", &[])
        .await
        .ok();

    // Create realistic tables with various geometry types and indexes
    for i in 0..count {
        let table_name = format!("bench_table_{}", i);

        // Drop if exists
        client
            .execute(&format!("DROP TABLE IF EXISTS {}", table_name), &[])
            .await
            .ok();

        // Create table with multiple geometry columns and metadata
        let geometry_type = match i % 4 {
            0 => "Point",
            1 => "LineString",
            2 => "Polygon",
            _ => "MultiPolygon",
        };

        let srid = match i % 3 {
            0 => 4326,
            1 => 3857,
            _ => 2154, // French projection for variety
        };

        client
            .execute(
                &format!(
                    "CREATE TABLE {} (
                        id SERIAL PRIMARY KEY,
                        geom geometry({}, {}),
                        name VARCHAR(255),
                        description TEXT,
                        category VARCHAR(100),
                        created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
                        properties JSONB
                    )",
                    table_name, geometry_type, srid
                ),
                &[],
            )
            .await
            .expect("Failed to create table");

        // Create spatial index
        client
            .execute(
                &format!(
                    "CREATE INDEX {}_geom_idx ON {} USING GIST (geom)",
                    table_name, table_name
                ),
                &[],
            )
            .await
            .expect("Failed to create spatial index");

        // Create attribute indexes for more realistic scenario
        if i % 2 == 0 {
            client
                .execute(
                    &format!(
                        "CREATE INDEX {}_category_idx ON {} (category)",
                        table_name, table_name
                    ),
                    &[],
                )
                .await
                .ok();
        }

        // Add some sample data to make bounds calculation more realistic
        let sample_geom = match geometry_type {
            "Point" => "ST_SetSRID(ST_MakePoint(-73.9857, 40.7484), {})",
            "LineString" => {
                "ST_SetSRID(ST_MakeLine(ST_MakePoint(-73.9857, 40.7484), ST_MakePoint(-73.9757, 40.7584)), {})"
            }
            "Polygon" => "ST_SetSRID(ST_MakeEnvelope(-74.0, 40.7, -73.9, 40.8), {})",
            _ => "ST_SetSRID(ST_MakeEnvelope(-74.0, 40.7, -73.9, 40.8), {})",
        };

        client
            .execute(
                &format!(
                    "INSERT INTO {} (geom, name, category, properties) VALUES ({}, 'Sample {}', 'category_{}', '{{}}'::jsonb)",
                    table_name,
                    sample_geom.replace("{}", &srid.to_string()),
                    i,
                    i % 5
                ),
                &[],
            )
            .await
            .expect("Failed to insert sample data");
    }

    // Analyze tables for better query planning
    client.execute("ANALYZE", &[]).await.ok();

    config
}

/// Setup database with realistic functions
async fn setup_functions(count: usize) -> PostgresConfig {
    let database_url = get_database_url();

    let config = PostgresConfig {
        connection_string: Some(database_url.clone()),
        ..Default::default()
    };

    let pool = PostgresPool::new(&database_url, None, None, None, 10)
        .await
        .expect("Failed to create pool");

    let client = pool.get().await.expect("Failed to get client");

    // Ensure PostGIS extension exists
    client
        .execute("CREATE EXTENSION IF NOT EXISTS postgis", &[])
        .await
        .ok();

    // Create realistic tile-serving functions
    for i in 0..count {
        let func_name = format!("bench_func_{}", i);

        // Drop if exists (handle both signatures)
        client
            .execute(
                &format!(
                    "DROP FUNCTION IF EXISTS {}(integer, integer, integer) CASCADE",
                    func_name
                ),
                &[],
            )
            .await
            .ok();
        client
            .execute(
                &format!(
                    "DROP FUNCTION IF EXISTS {}(integer, integer, integer, json) CASCADE",
                    func_name
                ),
                &[],
            )
            .await
            .ok();

        // Create realistic MVT-returning functions
        // Mix different function patterns that Martin might encounter
        let create_sql = match i % 4 {
            0 => {
                // Simple function without query param
                format!(
                    "CREATE FUNCTION {}(z integer, x integer, y integer)
                     RETURNS bytea AS $$
                     DECLARE
                       result bytea;
                     BEGIN
                       -- Simulate MVT generation with ST_AsMVT
                       SELECT ST_AsMVT(q, '{}', 4096, 'geom')
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
                     $$ LANGUAGE plpgsql IMMUTABLE STRICT PARALLEL SAFE",
                    func_name, func_name
                )
            }
            1 => {
                // Function with query param
                format!(
                    "CREATE FUNCTION {}(z integer, x integer, y integer, query_params json)
                     RETURNS bytea AS $$
                     DECLARE
                       result bytea;
                       filter_value text;
                     BEGIN
                       -- Extract filter from query params
                       filter_value := COALESCE(query_params->>'filter', '');

                       SELECT ST_AsMVT(q, '{}', 4096, 'geom')
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
                     $$ LANGUAGE plpgsql IMMUTABLE PARALLEL SAFE",
                    func_name, func_name
                )
            }
            2 => {
                // Function returning record with hash (for ETag support)
                format!(
                    "CREATE FUNCTION {}(z integer, x integer, y integer)
                     RETURNS TABLE(mvt bytea, hash text) AS $$
                     DECLARE
                       tile_data bytea;
                     BEGIN
                       SELECT ST_AsMVT(q, '{}', 4096, 'geom')
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
                     $$ LANGUAGE plpgsql IMMUTABLE STRICT PARALLEL SAFE",
                    func_name, func_name
                )
            }
            _ => {
                // Complex function with multiple CTEs (common pattern in production)
                format!(
                    "CREATE FUNCTION {}(z integer, x integer, y integer, query json)
                     RETURNS bytea AS $$
                     DECLARE
                       result bytea;
                       bbox geometry;
                     BEGIN
                       -- Get tile bbox
                       bbox := ST_TileEnvelope(z, x, y);

                       -- Complex query with CTEs
                       WITH filtered AS (
                         SELECT ST_MakePoint(0, 0) as geom, 'test' as name
                       ),
                       transformed AS (
                         SELECT
                           ST_AsMVTGeom(
                             ST_Transform(geom, 3857),
                             bbox,
                             4096, 64, true
                           ) AS geom,
                           name
                         FROM filtered
                       )
                       SELECT ST_AsMVT(transformed, '{}', 4096, 'geom')
                       INTO result
                       FROM transformed
                       WHERE geom IS NOT NULL;

                       RETURN COALESCE(result, '\\x00'::bytea);
                     END;
                     $$ LANGUAGE plpgsql STABLE PARALLEL SAFE",
                    func_name, func_name
                )
            }
        };

        client
            .execute(&create_sql, &[])
            .await
            .expect("Failed to create function");

        // Add comment/documentation to some functions (Martin reads these)
        if i % 3 == 0 {
            let comment = format!(
                "COMMENT ON FUNCTION {} IS 'Benchmark test function {} - returns MVT tiles'",
                if i % 2 == 0 {
                    format!("{}(integer, integer, integer)", func_name)
                } else {
                    format!("{}(integer, integer, integer, json)", func_name)
                },
                i
            );
            client.execute(&comment, &[]).await.ok();
        }
    }

    config
}

async fn discover_tables(config: &PostgresConfig) {
    let builder = PostgresAutoDiscoveryBuilder::new(config, IdResolver::default())
        .await
        .expect("Failed to create builder");

    let _tables = builder
        .instantiate_tables()
        .await
        .expect("Failed to discover tables");
}

async fn discover_functions(config: &PostgresConfig) {
    let builder = PostgresAutoDiscoveryBuilder::new(config, IdResolver::default())
        .await
        .expect("Failed to create builder");

    let _functions = builder
        .instantiate_functions()
        .await
        .expect("Failed to discover functions");
}

fn bench_table_discovery(c: &mut Criterion) {
    init_crypto();
    let runtime = tokio::runtime::Runtime::new().unwrap();

    // Clean up before starting
    runtime.block_on(cleanup_database());

    let mut group = c.benchmark_group("table_discovery");

    for size in SIZES {
        let config = runtime.block_on(setup_tables(*size));

        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, _| {
            b.to_async(&runtime).iter(|| discover_tables(&config));
        });
    }

    group.finish();
}

fn bench_function_discovery(c: &mut Criterion) {
    init_crypto();
    let runtime = tokio::runtime::Runtime::new().unwrap();

    // Clean up before starting
    runtime.block_on(cleanup_database());

    let mut group = c.benchmark_group("function_discovery");

    for size in SIZES {
        let config = runtime.block_on(setup_functions(*size));

        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, _| {
            b.to_async(&runtime).iter(|| discover_functions(&config));
        });
    }

    group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default().with_profiler(PProfProfiler::new(1000, Output::Flamegraph(None)));
    targets = bench_table_discovery, bench_function_discovery
}

criterion_main!(benches);
