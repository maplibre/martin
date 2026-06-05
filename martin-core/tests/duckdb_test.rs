#![cfg(feature = "duckdb")]

use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};

use duckdb::{Connection, params};
use futures::future::join_all;
use martin_core::CacheZoomRange;
use martin_core::tiles::duckdb::{
    DuckDBError, DuckDBPool, DuckDBResult, DuckDBSource, DuckDBSqlInfo,
};
use martin_core::tiles::{Source as _};
use martin_tile_utils::{Encoding, Format, TileCoord, TileInfo};
use tempfile::TempDir;
use tilejson::tilejson;

const SOURCE_ID: &str = "duckdb-test-source";
const POOL_ID: &str = "duckdb-test-pool";
const XYZ: TileCoord = TileCoord { z: 3, x: 4, y: 5 };

struct TestDatabase {
    _dir: TempDir,
    path: PathBuf,
}

impl TestDatabase {
    fn new() -> Self {
        let dir = TempDir::new().expect("temporary DuckDB directory");
        let path = dir.path().join("tiles.duckdb");
        let conn = Connection::open(&path).expect("writable DuckDB database");
        conn.execute_batch("INSTALL spatial;")
            .expect("spatial extension installed");
        conn.execute_batch("LOAD spatial;")
            .expect("spatial extension loaded");
        conn.execute_batch(
            "
CREATE TABLE tiles (
    z SMALLINT,
    x BIGINT,
    y BIGINT,
    tile BLOB
);

CREATE TABLE mvt_points (
    id INTEGER,
    name VARCHAR,
    geom GEOMETRY
);
",
        )
        .expect("tiles table created");
        conn.execute(
            "INSERT INTO tiles VALUES (?, ?, ?, ?)",
            params![3_i16, 4_i64, 5_i64, b"tile-data".to_vec()],
        )
        .expect("tile row inserted");
        conn.execute(
            "INSERT INTO tiles VALUES (?, ?, ?, ?)",
            params![6_i16, 7_i64, 8_i64, Option::<Vec<u8>>::None],
        )
        .expect("null tile row inserted");
        conn.execute(
            "INSERT INTO mvt_points VALUES (?, ?, ST_Point(?, ?))",
            params![1_i32, "origin", 0.0_f64, 0.0_f64],
        )
        .expect("MVT point row inserted");
        drop(conn);

        Self { _dir: dir, path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

//creates a duckdb file pool for testing
fn create_file_pool(path: &Path, pool_size: usize) -> DuckDBPool {
    DuckDBPool::new_database_file(
        POOL_ID.to_string(),
        path.to_path_buf(),
        pool_size,
        NonZeroUsize::new(4),
        NonZeroUsize::new(1024),
    )
    .expect("test pool created")

}
//duckdb tile error helper function
fn map_duckdb_error(e: duckdb::Error) -> DuckDBError {
    DuckDBError::GetTileError(Box::new(e), POOL_ID.to_string(), XYZ)
}

// a simple query for testing purposes
fn row_count(conn: &mut Connection) -> DuckDBResult<i64> {
    conn.query_row("SELECT COUNT(*) FROM tiles;", [], |row| row.get(0))
        .map_err(map_duckdb_error)
}

// creates a duckdb file source for testing
fn create_source(path: &Path, sql_query: &str) -> DuckDBSource {
    DuckDBSource::new(
        SOURCE_ID.to_string(),
        DuckDBSqlInfo::new(sql_query.to_string(), false, "z, x, y".to_string()),
        tilejson! {
            "http://example.test/tiles/{z}/{x}/{y}.mvt".to_string(),
            minzoom: 1,
            maxzoom: 10,
            name: "DuckDB Test".to_string(),
        },
        create_file_pool(path, 2),
        TileInfo::new(Format::Mvt, Encoding::Uncompressed),
        CacheZoomRange::new(Some(1), Some(8)),
    )
}


#[tokio::test]
async fn database_file_pool_creation() {
    let db = TestDatabase::new();
    let pool = create_file_pool(db.path(), 2);
    assert_eq!(pool.get_id(), POOL_ID);
}
#[tokio::test(flavor = "multi_thread")]
async fn pool_reuses_connections_after_successful_queries() {
    let db = TestDatabase::new();
    let pool = create_file_pool(db.path(), 1);

    pool.generate_tile(|conn| {
        conn.execute_batch(
            "
CREATE TEMP TABLE connection_reuse_marker (value INTEGER);
INSERT INTO connection_reuse_marker VALUES (42);
",
        )
        .map_err(map_duckdb_error)
    })
    .await
    .expect("temporary marker created on pooled connection");

    let marker: i32 = pool
        .generate_tile(|conn| {
            conn.query_row("SELECT value FROM connection_reuse_marker;", [], |row| {
                row.get::<_, i32>(0)
            })
            .map_err(map_duckdb_error)
        })
        .await
        .expect("temporary marker read from reused pooled connection");

    assert_eq!(marker, 42);
}

#[tokio::test(flavor = "multi_thread")]
async fn pool_runs_concurrent_queries() {
    let db = TestDatabase::new();
    let pool = create_file_pool(db.path(), 2);

    let results = join_all((0..5).map(|_| {
        let pool = pool.clone();
        async move { pool.generate_tile(row_count).await }
    }))
    .await;

    for result in results {
        assert_eq!(result.expect("concurrent row count query"), 2);
    }
}

#[tokio::test]
async fn pool_propagates_connection_work_errors() {
    let db = TestDatabase::new();
    let pool = create_file_pool(db.path(), 2);

    let err = pool
        .generate_tile(|conn| {
            conn.execute_batch("THIS IS NOT SQL")
                .map_err(map_duckdb_error)?;
            Ok(())
        })
        .await
        .expect_err("invalid SQL should be returned from generate_tile");

    match err {
        DuckDBError::GetTileError(_, source_id, xyz) => {
            assert_eq!(source_id, POOL_ID);
            assert_eq!(xyz, XYZ);
        }
        other => panic!("expected GetTileError, got {other:?}"),
    }
}

#[tokio::test]
async fn database_file_pool_is_read_only() {
    let db = TestDatabase::new();
    let pool = create_file_pool(db.path(), 2);

    let err = pool
        .generate_tile(|conn| {
            conn.execute_batch("CREATE TABLE writes_are_rejected (id INTEGER)")
                .map_err(map_duckdb_error)?;
            Ok(())
        })
        .await
        .expect_err("read-only pool should reject writes");

    match err {
        DuckDBError::GetTileError(_, source_id, xyz) => {
            assert_eq!(source_id, POOL_ID);
            assert_eq!(xyz, XYZ);
        }
        other => panic!("expected GetTileError, got {other:?}"),
    }
}

// ============================================================================
// Source Tests
// ============================================================================

#[tokio::test(flavor = "multi_thread")]
async fn source_serves_tiles_and_cloned_source_remains_usable() {
    let db = TestDatabase::new();
    let source = create_source(
        db.path(),
        "SELECT tile FROM tiles WHERE z = ? AND x = ? AND y = ?",
    );

    assert_eq!(source.get_id(), SOURCE_ID);
    assert_eq!(source.get_tilejson().name.as_deref(), Some("DuckDB Test"));
    assert_eq!(
        source.get_tile_info(),
        TileInfo::new(Format::Mvt, Encoding::Uncompressed)
    );
    assert_eq!(source.cache_zoom(), CacheZoomRange::new(Some(1), Some(8)));
    assert!(!source.support_url_query());
    assert!(!source.benefits_from_concurrent_scraping());

    let tile = source.get_tile(XYZ, None).await.expect("source tile");
    assert_eq!(tile, b"tile-data");

    let cloned = source.clone_source();
    let cloned_tile = cloned
        .get_tile(XYZ, None)
        .await
        .expect("cloned source tile");
    assert_eq!(cloned_tile, b"tile-data");
}

#[tokio::test(flavor = "multi_thread")]
async fn source_returns_empty_tiles_for_missing_or_null_rows() {
    let db = TestDatabase::new();
    let source = create_source(
        db.path(),
        "SELECT tile FROM tiles WHERE z = ? AND x = ? AND y = ?",
    );

    let missing = source
        .get_tile(TileCoord { z: 0, x: 0, y: 0 }, None)
        .await
        .expect("missing tile query");
    let null = source
        .get_tile(TileCoord { z: 6, x: 7, y: 8 }, None)
        .await
        .expect("null tile query");

    assert!(missing.is_empty());
    assert!(null.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn source_generates_mvt_with_duckdb_spatial_functions() {
    let db = TestDatabase::new();
    let source = create_source(
        db.path(),
        "
WITH bounds AS (
    SELECT ST_TileEnvelope(
        CAST(? AS INTEGER),
        CAST(? AS INTEGER),
        CAST(? AS INTEGER)
    ) AS geom
),
features AS (
    SELECT struct_pack(
        geom := ST_AsMVTGeom(mvt_points.geom, ST_Extent(bounds.geom), 4096, 256, TRUE),
        id := mvt_points.id,
        name := mvt_points.name
    ) AS feature
    FROM mvt_points, bounds
    WHERE ST_Intersects(mvt_points.geom, bounds.geom)
)
SELECT ST_AsMVT(feature, 'mvt_points', 4096, 'geom', 'id')
FROM features
",
    );

    let tile = source
        .get_tile(TileCoord { z: 0, x: 0, y: 0 }, None)
        .await
        .expect("DuckDB spatial MVT tile");

    assert!(!tile.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn end_to_end_tile_retrieval_at_multiple_zoom_levels() {
    let db = TestDatabase::new();
    let conn = Connection::open(db.path()).expect("read database");

    // Insert test data at multiple zoom levels
    conn.execute(
        "INSERT INTO tiles VALUES (?, ?, ?, ?)",
        params![0_i16, 0_i64, 0_i64, b"z0-tile".to_vec()],
    )
    .expect("z0 tile inserted");
    conn.execute(
        "INSERT INTO tiles VALUES (?, ?, ?, ?)",
        params![1_i16, 0_i64, 0_i64, b"z1-tile-0-0".to_vec()],
    )
    .expect("z1 tile 0,0 inserted");
    conn.execute(
        "INSERT INTO tiles VALUES (?, ?, ?, ?)",
        params![1_i16, 1_i64, 1_i64, b"z1-tile-1-1".to_vec()],
    )
    .expect("z1 tile 1,1 inserted");
    drop(conn);

    let source = create_source(
        db.path(),
        "SELECT tile FROM tiles WHERE z = ? AND x = ? AND y = ?",
    );

    // Test zoom level 0
    let tile_z0 = source
        .get_tile(TileCoord { z: 0, x: 0, y: 0 }, None)
        .await
        .expect("z0 tile");
    assert_eq!(tile_z0, b"z0-tile");

    // Test zoom level 1
    let tile_z1_00 = source
        .get_tile(TileCoord { z: 1, x: 0, y: 0 }, None)
        .await
        .expect("z1 tile 0,0");
    assert_eq!(tile_z1_00, b"z1-tile-0-0");

    let tile_z1_11 = source
        .get_tile(TileCoord { z: 1, x: 1, y: 1 }, None)
        .await
        .expect("z1 tile 1,1");
    assert_eq!(tile_z1_11, b"z1-tile-1-1");

    // Test original z3 tile
    let tile_z3 = source.get_tile(XYZ, None).await.expect("z3 tile");
    assert_eq!(tile_z3, b"tile-data");
}

#[tokio::test(flavor = "multi_thread")]
async fn concurrent_tile_requests_from_different_coordinates() {
    let db = TestDatabase::new();
    let conn = Connection::open(db.path()).expect("read database");

    // Insert tiles at different coordinates
    for z in 0_i16..3 {
        for x in 0_i64..4 {
            for y in 0_i64..4 {
                let data = format!("tile_z{z}_x{x}_y{y}").into_bytes();
                conn.execute(
                    "INSERT INTO tiles VALUES (?, ?, ?, ?)",
                    params![z, x, y, data],
                )
                .expect("tile inserted");
            }
        }
    }
    drop(conn);

    let source = create_source(
        db.path(),
        "SELECT tile FROM tiles WHERE z = ? AND x = ? AND y = ?",
    );

    // Create concurrent requests for different tiles
    let tasks = [
        TileCoord { z: 0, x: 0, y: 0 },
        TileCoord { z: 1, x: 1, y: 1 },
        TileCoord { z: 2, x: 2, y: 2 },
        TileCoord { z: 2, x: 3, y: 0 },
        TileCoord { z: 1, x: 0, y: 3 },
    ];

    let results = join_all(tasks.iter().map(|&coord| {
        let source = source.clone_source();
        async move {
            source
                .get_tile(coord, None)
                .await
                .expect("concurrent tile request")
        }
    }))
    .await;

    // Verify each returned tile matches expected content
    for (i, coord) in tasks.iter().enumerate() {
        let expected = format!("tile_z{}_x{}_y{}", coord.z, coord.x, coord.y);
        assert_eq!(
            results[i],
            expected.as_bytes(),
            "tile mismatch at coord {i}",
        );
    }
}