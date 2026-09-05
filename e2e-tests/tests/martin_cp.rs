//! The `martin-cp` bulk tile copier.

use std::fs;
use std::path::Path;

use martin_e2e_tests::{MartinCp, MbtilesCli, mbtiles_fixture, metadata, summary, temp_dir};
use rstest::rstest;
use serde_json::{Value, json};

const GENERATOR: &str = "generator=martin-cp v0.0.0";

async fn validate(path: &Path) {
    MbtilesCli::new("validate").arg(path).run().await;
}

#[tokio::test]
async fn copies_the_only_source_when_none_is_named() {
    let dir = temp_dir();
    let source = mbtiles_fixture(dir.path(), "world_cities").await;
    let output = dir.path().join("out.mbtiles");

    MartinCp::new()
        .arg(&source)
        .arg("--output-file")
        .arg(&output)
        .arg("--mbtiles-type")
        .arg("flat")
        .arg("--min-zoom")
        .arg("0")
        .arg("--max-zoom")
        .arg("6")
        .arg("--bbox=-2,-1,142.84,45")
        .arg("--set-meta")
        .arg(GENERATOR)
        .run()
        .await;

    let summary = summary(&output).run_json().await;
    assert_eq!(summary["tile_count"], 8);
    assert_eq!(summary["min_zoom"], 0);
    assert_eq!(summary["max_zoom"], 6);

    let metadata = metadata(&output).await;
    assert_eq!(
        metadata["name"], "Major cities from Natural Earth data",
        "the copy did not carry over the source metadata: {metadata:?}"
    );
    assert_eq!(metadata["generator"], "martin-cp v0.0.0");
    assert_eq!(metadata["format"], "pbf");
    validate(&output).await;
}

#[rstest]
#[case::flat("flat", json!("Flat"))]
#[case::flat_with_hash("flat-with-hash", json!("FlatWithHash"))]
#[case::normalized("normalized", json!({"Normalized": {"hash_view": true, "schema": "Hash"}}))]
#[tokio::test]
async fn writes_the_requested_schema(#[case] mbtiles_type: &str, #[case] expected: Value) {
    let dir = temp_dir();
    let source = mbtiles_fixture(dir.path(), "world_cities").await;
    let output = dir.path().join("out.mbtiles");

    MartinCp::new()
        .arg(&source)
        .arg("--output-file")
        .arg(&output)
        .arg("--mbtiles-type")
        .arg(mbtiles_type)
        .arg("--min-zoom")
        .arg("0")
        .arg("--max-zoom")
        .arg("1")
        .run()
        .await;

    assert_eq!(summary(&output).run_json().await["mbt_type"], expected);
    validate(&output).await;
}

#[tokio::test]
async fn copies_a_raster_source() {
    let dir = temp_dir();
    let source = mbtiles_fixture(dir.path(), "geography-class-png").await;
    let output = dir.path().join("out.mbtiles");

    MartinCp::new()
        .arg(&source)
        .arg("--output-file")
        .arg(&output)
        .arg("--mbtiles-type")
        .arg("normalized")
        .arg("--min-zoom")
        .arg("0")
        .arg("--max-zoom")
        .arg("6")
        .arg("--set-meta")
        .arg(GENERATOR)
        .arg("--set-meta")
        .arg("name=normalized")
        .arg("--set-meta")
        .arg("center=0,0,0")
        .run()
        .await;

    assert_eq!(summary(&output).run_json().await["tile_count"], 6);

    let metadata = metadata(&output).await;
    assert_eq!(metadata["format"], "png");
    assert_eq!(metadata["name"], "normalized");
    assert_eq!(metadata["center"], "0,0,0");
    validate(&output).await;
}

#[tokio::test]
async fn saves_the_resolved_config() {
    let dir = temp_dir();
    let source = mbtiles_fixture(dir.path(), "world_cities").await;
    let output = dir.path().join("out.mbtiles");
    let config = dir.path().join("config.yaml");

    MartinCp::new()
        .arg(&source)
        .arg("--output-file")
        .arg(&output)
        .arg("--save-config")
        .arg(&config)
        .arg("--min-zoom")
        .arg("0")
        .arg("--max-zoom")
        .arg("0")
        .run()
        .await;

    let saved = fs::read_to_string(&config).expect("failed to read the saved config");
    assert_eq!(
        saved.trim_end(),
        format!("mbtiles:\n  sources:\n    world_cities: {}", source.display())
    );
}

#[cfg(feature = "test-pg")]
mod postgres {
    use std::fs;
    use std::path::Path;

    use martin_e2e_tests::{GZIP_MAGIC, MartinCp, gunzip, metadata, summary, temp_dir, tiles};
    use mlt_core::fast_mvt::MvtReaderRef;
    use serde_json::json;

    use crate::{GENERATOR, validate};

    /// The arguments shared by every copy from the test database.
    fn copy(output: &Path) -> MartinCp {
        MartinCp::new()
            .with_postgres()
            .arg("--default-srid")
            .arg("900913")
            .arg("--output-file")
            .arg(output)
            .arg("--concurrency")
            .arg("3")
            .arg("--set-meta")
            .arg(GENERATOR)
    }

    async fn lowest_zoom_tile(path: &Path) -> Vec<u8> {
        let (_, _, _, data) = tiles(path)
            .await
            .into_iter()
            .next()
            .expect("the copy wrote no tiles");
        data
    }

    fn layer_names(tile: &[u8]) -> Vec<String> {
        MvtReaderRef::new(tile)
            .expect("a tile is not a vector tile")
            .to_tile()
            .expect("a tile is not a decodable vector tile")
            .layers
            .into_iter()
            .map(|layer| layer.name)
            .collect()
    }

    #[tokio::test]
    async fn copies_a_table_source() {
        let dir = temp_dir();
        let output = dir.path().join("out.mbtiles");

        copy(&output)
            .arg("--source")
            .arg("table_source")
            .arg("--mbtiles-type")
            .arg("flat")
            .arg("--min-zoom")
            .arg("0")
            .arg("--max-zoom")
            .arg("6")
            .arg("--bbox=-2,-1,142.84,45")
            .run()
            .await;

        let summary = summary(&output).run_json().await;
        assert_eq!(summary["mbt_type"], json!("Flat"));
        assert_eq!(summary["tile_count"], 127);
        assert_eq!(summary["max_zoom"], 6);

        let metadata = metadata(&output).await;
        assert_eq!(metadata["format"], "pbf");
        assert_eq!(metadata["compression"], "gzip");
        assert_eq!(metadata["generator"], "martin-cp v0.0.0");

        let tile = lowest_zoom_tile(&output).await;
        assert!(tile.starts_with(&GZIP_MAGIC), "the tile is not gzipped");
        assert_eq!(layer_names(&gunzip(&tile)), ["table_source"]);
        validate(&output).await;
    }

    #[tokio::test]
    async fn copies_a_function_source_answering_a_url_query() {
        let dir = temp_dir();
        let output = dir.path().join("out.mbtiles");

        copy(&output)
            .arg("--source")
            .arg("function_zxy_query_test")
            .arg("--url-query")
            .arg("foo=bar&token=martin")
            .arg("--encoding")
            .arg("identity")
            .arg("--mbtiles-type")
            .arg("flat-with-hash")
            .arg("--min-zoom")
            .arg("0")
            .arg("--max-zoom")
            .arg("6")
            .arg("--bbox=-2,-1,142.84,45")
            .run()
            .await;

        assert_eq!(summary(&output).run_json().await["mbt_type"], json!("FlatWithHash"));

        let metadata = metadata(&output).await;
        assert_eq!(metadata["name"], "function_zxy_query_test");
        assert!(
            !metadata.contains_key("compression"),
            "`--encoding identity` recorded a compression: {metadata:?}"
        );

        let tile = lowest_zoom_tile(&output).await;
        assert!(!tile.starts_with(&GZIP_MAGIC), "the tile is gzipped");
        assert_eq!(layer_names(&tile), ["public.function_zxy_query_test"]);
        validate(&output).await;
    }

    #[tokio::test]
    async fn copies_every_layer_of_a_composite_source() {
        let dir = temp_dir();
        let output = dir.path().join("out.mbtiles");

        copy(&output)
            .arg("--source")
            .arg("table_source,function_zxy_query_test")
            .arg("--url-query")
            .arg("foo=bar&token=martin")
            .arg("--mbtiles-type")
            .arg("normalized")
            .arg("--min-zoom")
            .arg("0")
            .arg("--max-zoom")
            .arg("0")
            .arg("--bbox=-2,-1,142.84,45")
            .arg("--set-meta")
            .arg("name=composite")
            .run()
            .await;

        assert_eq!(metadata(&output).await["name"], "composite");
        let tile = gunzip(&lowest_zoom_tile(&output).await);
        assert_eq!(layer_names(&tile), ["table_source", "public.function_zxy_query_test"]);
        validate(&output).await;
    }

    #[tokio::test]
    async fn falls_back_to_the_source_bounds_without_a_bbox() {
        let dir = temp_dir();
        let bounded = dir.path().join("bounded.mbtiles");
        let unbounded = dir.path().join("unbounded.mbtiles");

        for (output, bbox) in [
            (&bounded, Some("--bbox=-2,-1,142.84131509869133,45")),
            (&unbounded, None),
        ] {
            let mut command = copy(output)
                .arg("--auto-bounds")
                .arg("calc")
                .arg("--source")
                .arg("table_source")
                .arg("--mbtiles-type")
                .arg("flat")
                .arg("--min-zoom")
                .arg("0")
                .arg("--max-zoom")
                .arg("6");
            if let Some(bbox) = bbox {
                command = command.arg(bbox);
            }
            command.run().await;
        }

        assert_eq!(tiles(&bounded).await, tiles(&unbounded).await);
    }

    #[tokio::test]
    async fn saves_the_resolved_config() {
        let dir = temp_dir();
        let output = dir.path().join("out.mbtiles");
        let config = dir.path().join("config.yaml");

        copy(&output)
            .arg("--save-config")
            .arg(&config)
            .arg("--source")
            .arg("table_source")
            .arg("--min-zoom")
            .arg("0")
            .arg("--max-zoom")
            .arg("0")
            .run()
            .await;

        let saved = fs::read_to_string(&config).expect("failed to read the saved config");
        assert!(
            saved.contains("    table_source:\n      schema: public\n      table: table_source\n"),
            "the saved config does not describe the copied table:\n{saved}"
        );
    }
}
