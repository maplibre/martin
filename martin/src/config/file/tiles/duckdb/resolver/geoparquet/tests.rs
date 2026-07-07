use martin_core::tiles::Source;
use martin_tile_utils::{Format, TileCoord};
use tilejson::Bounds;

use super::resolve_geoparquet_source;
use crate::config::file::CachePolicy;
use crate::config::file::tiles::duckdb::resolver::errors::GeoparquetError;
use crate::config::file::tiles::duckdb::sources::GeoParquetEntry;
use crate::test_support::duckdb::TestGeoParquet;

fn points_fixture() -> TestGeoParquet {
    TestGeoParquet::from_sql(
        "points.parquet",
        include_str!("../../../../../../../../tests/fixtures/duckdb/geoparquet_points.sql"),
        "points",
    )
}

#[tokio::test(flavor = "multi_thread")]
async fn resolve_geoparquet_source_auto_detects_metadata() {
    let fixture = points_fixture();
    let entry = GeoParquetEntry {
        geoparquet: fixture.path().to_path_buf(),
        minzoom: Some(0),
        maxzoom: Some(14),
        ..GeoParquetEntry::default()
    };
    let pool = fixture.query_pool("geoparquet-resolve", 1);

    let source = resolve_geoparquet_source(
        "buildings".to_string(),
        &entry,
        pool,
        CachePolicy::default(),
    )
    .await
    .expect("resolve geoparquet source");

    let tilejson = Source::get_tilejson(source.as_ref());
    assert_eq!(tilejson.name.as_deref(), Some("buildings"));
    assert_eq!(tilejson.minzoom, Some(0));
    assert_eq!(tilejson.maxzoom, Some(14));
    assert_eq!(tilejson.bounds, Some(Bounds::new(10.0, 20.0, 11.0, 21.0)));

    let layers = tilejson
        .vector_layers
        .as_ref()
        .expect("vector layers populated");
    assert_eq!(layers.len(), 1);
    assert_eq!(layers[0].id, "buildings");
    assert_eq!(
        layers[0].fields.keys().collect::<Vec<_>>(),
        vec!["id", "point_name"]
    );
    assert_eq!(source.get_tile_info().format, Format::Mvt);
}

#[tokio::test(flavor = "multi_thread")]
async fn resolve_geoparquet_source_serves_mvt_tile() {
    let fixture = points_fixture();
    let entry = GeoParquetEntry {
        geoparquet: fixture.path().to_path_buf(),
        srid: Some(4326),
        ..GeoParquetEntry::default()
    };
    let pool = fixture.query_pool("geoparquet-tile", 1);

    let source = resolve_geoparquet_source(
        "buildings".to_string(),
        &entry,
        pool,
        CachePolicy::default(),
    )
    .await
    .expect("resolve geoparquet source");

    let tile = source
        .get_tile(TileCoord { z: 0, x: 0, y: 0 }, None)
        .await
        .expect("MVT tile from resolved GeoParquet source");
    assert!(!tile.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn resolve_geoparquet_source_rejects_ambiguous_geometry_columns() {
    let fixture = TestGeoParquet::from_sql(
        "two_geoms.parquet",
        include_str!("../../../../../../../../tests/fixtures/duckdb/geoparquet_two_geoms.sql"),
        "two_geoms",
    );
    let entry = GeoParquetEntry {
        geoparquet: fixture.path().to_path_buf(),
        ..GeoParquetEntry::default()
    };
    let pool = fixture.query_pool("geoparquet-ambiguous", 1);

    let err = resolve_geoparquet_source(
        "ambiguous".to_string(),
        &entry,
        pool,
        CachePolicy::default(),
    )
    .await
    .expect_err("ambiguous geometry columns");

    assert!(matches!(err, GeoparquetError::AmbiguousGeometryColumn(..)));
}

#[tokio::test(flavor = "multi_thread")]
async fn resolve_geoparquet_source_rejects_missing_id_column() {
    let fixture = points_fixture();
    let entry = GeoParquetEntry {
        geoparquet: fixture.path().to_path_buf(),
        id_column: Some("missing_id".to_string()),
        srid: Some(4326),
        ..GeoParquetEntry::default()
    };
    let pool = fixture.query_pool("geoparquet-missing-id", 1);

    let err = resolve_geoparquet_source(
        "missing-id".to_string(),
        &entry,
        pool,
        CachePolicy::default(),
    )
    .await
    .expect_err("missing id column");

    assert!(matches!(err, GeoparquetError::IdColumnNotFound(..)));
}
