//! The `martin cp` bulk tile copier.

use std::fs;
use std::path::Path;

use martin_e2e_tests::{
    MartinCp, MbtilesCli, mbtiles_fixture, metadata_listing, summary, summary_filters, temp_dir,
    tile_listing,
};
use rstest::rstest;
use serde_json::{Value, json};

const GENERATOR: &str = "generator=martin cp v0.0.0";

async fn validate(path: &Path) {
    MbtilesCli::new("validate").arg(path).run().await;
}

/// Insta filters that round every float to ten digits and drop trailing whitespace: martin
/// computes the bounds and the center by trigonometry, so their last digits differ between
/// machines, and the repository strips trailing whitespace from every committed file, so a
/// snapshot that recorded it could never match again.
fn snapshot_filters() -> Vec<(&'static str, &'static str)> {
    vec![(r"(-?\d+\.\d{10})\d+", "$1"), (r"(?m)[ \t]+$", "")]
}

#[rstest]
#[case("png", "invalid value 'png'")]
#[case("jpeg", "invalid value 'jpeg'")]
#[cfg_attr(not(feature = "test-mlt-v2"), case("mltv2", "invalid value 'mltv2'"))]
#[cfg_attr(not(feature = "test-mlt-v2"), case("mlt2", "invalid value 'mlt2'"))]
#[tokio::test]
async fn refuses_a_format_it_cannot_write(#[case] format: &str, #[case] expected: &str) {
    let dir = temp_dir();
    let source = mbtiles_fixture(dir.path(), "world_cities").await;

    let log = MartinCp::new()
        .arg(&source)
        .arg("--output-file")
        .arg(dir.path().join("out.mbtiles"))
        .arg("--format")
        .arg(format)
        .arg("--min-zoom")
        .arg("0")
        .arg("--max-zoom")
        .arg("0")
        .run_expecting_failure()
        .await;
    assert!(log.contains(expected), "`--format {format}` said:\n{log}");
}

#[rstest]
#[case("mlt")]
#[case("mlt1")]
#[case("mltv1")]
#[tokio::test]
async fn copies_as_mlt_under_every_v1_spelling(#[case] format: &str) {
    let dir = temp_dir();
    let source = mbtiles_fixture(dir.path(), "world_cities").await;
    let output = dir.path().join("out.mbtiles");

    MartinCp::new()
        .arg(&source)
        .arg("--output-file")
        .arg(&output)
        .arg("--format")
        .arg(format)
        .arg("--mbtiles-type")
        .arg("flat")
        .arg("--min-zoom")
        .arg("0")
        .arg("--max-zoom")
        .arg("0")
        .run()
        .await;

    let metadata = metadata_listing(&output).await;
    assert!(
        metadata.contains("mlt"),
        "`--format {format}` wrote {metadata}"
    );
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
    let metadata = metadata_listing(&output).await;
    insta::with_settings!({filters => summary_filters()}, {
        insta::assert_json_snapshot!("only_source_summary", summary);
    });
    insta::assert_snapshot!("only_source_tiles", tile_listing(&output).await);
    insta::with_settings!({filters => snapshot_filters()}, {
        insta::assert_snapshot!("only_source_metadata", metadata);
    });
    validate(&output).await;
}

#[rstest]
#[case::flat(Some("flat"), json!("Flat"))]
#[case::flat_with_hash(Some("flat-with-hash"), json!("FlatWithHash"))]
#[case::normalized(Some("normalized"), json!({"Normalized": {"hash_view": false, "schema": "DedupId"}}))]
#[case::default(None, json!({"Normalized": {"hash_view": false, "schema": "DedupId"}}))]
#[tokio::test]
async fn writes_the_requested_schema(#[case] mbtiles_type: Option<&str>, #[case] expected: Value) {
    let dir = temp_dir();
    let source = mbtiles_fixture(dir.path(), "world_cities").await;
    let output = dir.path().join("out.mbtiles");

    let mut cp = MartinCp::new()
        .arg(&source)
        .arg("--output-file")
        .arg(&output);
    if let Some(mbtiles_type) = mbtiles_type {
        cp = cp.arg("--mbtiles-type").arg(mbtiles_type);
    }
    cp.arg("--min-zoom")
        .arg("0")
        .arg("--max-zoom")
        .arg("1")
        .run()
        .await;

    let summary = summary(&output).run_json().await;
    assert_eq!(summary["mbt_type"], expected);
    assert_eq!(summary["tile_count"], 2);
    insta::assert_snapshot!(tile_listing(&output).await, @r"
    0/0/0 1107 bytes
    1/0/0 20 bytes
    ");
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

    let summary = summary(&output).run_json().await;
    let metadata = metadata_listing(&output).await;
    insta::with_settings!({filters => summary_filters()}, {
        insta::assert_json_snapshot!("raster_summary", summary);
    });
    insta::assert_snapshot!("raster_tiles", tile_listing(&output).await);
    insta::with_settings!({filters => snapshot_filters()}, {
        insta::assert_snapshot!("raster_metadata", metadata);
    });
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
    let saved = saved.replace(&source.display().to_string(), "[SOURCE]");
    insta::assert_snapshot!(saved.trim_end(), @"
    mbtiles:
      sources:
        world_cities: [SOURCE]
    ");
}

#[cfg(feature = "test-pg")]
mod postgres {
    use std::collections::{BTreeMap, BTreeSet};
    use std::fs;
    use std::path::Path;

    use martin_e2e_tests::{
        GZIP_MAGIC, Martin, MartinCp, gunzip, metadata_listing, mlt_dump,
        mlt_dump_ignoring_ring_start, mlt_layers, mvt_dump, rings_from_smallest_vertex, summary,
        summary_filters, temp_dir, tile_listing, tiles,
    };
    use mlt_core::TileLayer;

    use crate::{GENERATOR, snapshot_filters, validate};

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

    /// The copy's lowest tile as text, whether or not the copy gzipped it.
    async fn lowest_zoom_dump(path: &Path) -> String {
        mvt_dump(&gunzip(&lowest_zoom_tile(path).await))
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
        let metadata = metadata_listing(&output).await;
        insta::with_settings!({filters => summary_filters()}, {
            insta::assert_json_snapshot!("table_source_summary", summary);
        });
        insta::with_settings!({filters => snapshot_filters()}, {
            insta::assert_snapshot!("table_source_metadata", metadata);
        });

        assert!(
            lowest_zoom_tile(&output).await.starts_with(&GZIP_MAGIC),
            "the tile is not gzipped"
        );
        insta::assert_snapshot!("table_source_0_0_0", lowest_zoom_dump(&output).await);
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

        let summary = summary(&output).run_json().await;
        let metadata = metadata_listing(&output).await;
        insta::with_settings!({filters => summary_filters()}, {
            insta::assert_json_snapshot!("function_source_summary", summary);
        });
        insta::with_settings!({filters => snapshot_filters()}, {
            insta::assert_snapshot!("function_source_metadata", metadata);
        });
        assert!(
            !metadata.contains("compression ="),
            "`--encoding identity` recorded a compression:\n{metadata}"
        );

        assert!(
            !lowest_zoom_tile(&output).await.starts_with(&GZIP_MAGIC),
            "the tile is gzipped"
        );
        insta::assert_snapshot!("function_source_0_0_0", lowest_zoom_dump(&output).await);
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

        let metadata = metadata_listing(&output).await;
        insta::with_settings!({filters => snapshot_filters()}, {
            insta::assert_snapshot!("composite_metadata", metadata);
        });
        insta::assert_snapshot!("composite_0_0_0", lowest_zoom_dump(&output).await);
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
        insta::assert_snapshot!("source_bounds_tiles", tile_listing(&unbounded).await);
    }

    /// The config the MLT test below drives: one table, with every property type martin serves.
    const MEASURED: &str = "
postgres:
  connection_string: ${DATABASE_URL}
  pool_size: 2
  auto_publish: false
  tables:
    measured_shapes:
      schema: public
      table: measured_shapes
      srid: 4326
      geometry_column: geom
      id_column: feat_id
      bounds: [-180.0, -90.0, 180.0, 90.0]
      properties:
        big: int8
        small: int2
        signed: int4
        flag: bool
        single: float4
        double: float8
        label: text
        code: varchar
        never_set: int4
";

    /// `--format mlt` builds the tile from the table's rows instead of from an MVT tile, and the
    /// two describe the same features. The M ordinate the geometries carry is dropped either way.
    #[tokio::test]
    async fn copies_a_table_as_mlt_without_the_mvt_round_trip() {
        let dir = temp_dir();
        let config = dir.path().join("config.yaml");
        fs::write(&config, MEASURED).expect("failed to write the config");
        let output = dir.path().join("measured.mbtiles");

        let log = MartinCp::new()
            .with_postgres()
            .env("RUST_LOG", "martin_core::tiles::postgres=debug")
            .arg("--config")
            .arg(&config)
            .arg("--source")
            .arg("measured_shapes")
            .arg("--format")
            .arg("mlt")
            .arg("--encoding")
            .arg("identity")
            .arg("--output-file")
            .arg(&output)
            .arg("--mbtiles-type")
            .arg("flat")
            .arg("--min-zoom")
            .arg("0")
            .arg("--max-zoom")
            .arg("0")
            .run()
            .await;
        assert!(
            log.contains("ST_AsBinary("),
            "the copy did not run the row-per-feature query:\n{log}"
        );
        assert!(
            !log.contains("ST_AsMVT(tile"),
            "the copy still built an MVT tile:\n{log}"
        );

        let mut martin = Martin::builder()
            .with_postgres()
            .config(MEASURED)
            .start()
            .await
            .expect("failed to start martin");
        let response = martin
            .get_with_headers(
                "/measured_shapes/0/0/0",
                &[("Accept", "application/vnd.maplibre-tile")],
            )
            .await;
        assert_eq!(response.status(), 200);
        let round_trip = response.mlt();
        martin.stop().await;

        let direct = mlt_layers(&lowest_zoom_tile(&output).await);
        assert!(
            !direct[0].property_names().contains(&"never_set".to_owned()),
            "a column that is NULL for every feature must not become a layer column: {:?}",
            direct[0].property_names()
        );
        insta::assert_snapshot!("measured_shapes_0_0_0", mlt_dump(&direct));
        assert_eq!(mlt_dump(&direct), mlt_dump(&round_trip));
    }

    /// The config for the table holding every geometry type in every `PostGIS` dimension.
    const DIMENSIONED: &str = "
postgres:
  connection_string: ${DATABASE_URL}
  pool_size: 2
  auto_publish: false
  tables:
    dimensioned_shapes:
      schema: public
      table: dimensioned_shapes
      srid: 4326
      geometry_column: geom
      id_column: feat_id
      bounds: [-180.0, -90.0, 180.0, 90.0]
      properties:
        dims: text
        kind: text
";

    /// Copies `dimensioned_shapes` at zoom 0 in `format` and returns the tile's layers.
    async fn copy_dimensioned(format: &str) -> Vec<TileLayer> {
        let dir = temp_dir();
        let config = dir.path().join("config.yaml");
        fs::write(&config, DIMENSIONED).expect("failed to write the config");
        let output = dir.path().join("dimensioned.mbtiles");

        let log = MartinCp::new()
            .with_postgres()
            .env("RUST_LOG", "martin=warn,martin_core::tiles::postgres=debug")
            .arg("--config")
            .arg(&config)
            .arg("--source")
            .arg("dimensioned_shapes")
            .arg("--format")
            .arg(format)
            .arg("--encoding")
            .arg("identity")
            .arg("--output-file")
            .arg(&output)
            .arg("--mbtiles-type")
            .arg("flat")
            .arg("--min-zoom")
            .arg("0")
            .arg("--max-zoom")
            .arg("0")
            .run()
            .await;
        assert!(
            log.contains("ST_AsBinary("),
            "the copy did not run the row-per-feature query:\n{log}"
        );
        assert!(
            !log.contains("through MVT"),
            "the copy fell back off the row-per-feature path:\n{log}"
        );
        mlt_layers(&lowest_zoom_tile(&output).await)
    }

    /// Z, M and ZM geometries of every type take the row path like their XY twins, land on the
    /// same tile coordinates, and match what the MVT round-trip serves. A v1 tile has nowhere to
    /// keep Z or M, so it holds x and y alone.
    #[tokio::test]
    async fn copies_every_geometry_type_in_every_dimension_as_mlt() {
        let direct = copy_dimensioned("mlt").await;

        let mut martin = Martin::builder()
            .with_postgres()
            .config(DIMENSIONED)
            .start()
            .await
            .expect("failed to start martin");
        let response = martin
            .get_with_headers(
                "/dimensioned_shapes/0/0/0",
                &[("Accept", "application/vnd.maplibre-tile")],
            )
            .await;
        assert_eq!(response.status(), 200);
        let round_trip = response.mlt();
        martin.stop().await;

        let layer = &direct[0];
        let kind = layer
            .property_names()
            .iter()
            .position(|name| name == "kind")
            .expect("the layer has no kind column");
        let mut geometries = BTreeMap::<String, BTreeSet<String>>::new();
        for feature in layer.features() {
            geometries
                .entry(format!("{:?}", feature.properties()[kind]))
                .or_default()
                .insert(format!(
                    "{:?}",
                    rings_from_smallest_vertex(feature.geometry())
                ));
        }
        assert_eq!(
            geometries.len(),
            7,
            "a geometry type is missing: {geometries:#?}"
        );
        assert!(
            geometries.values().all(|shapes| shapes.len() == 1),
            "a Z or M ordinate moved a geometry: {geometries:#?}"
        );
        assert_eq!(layer.features().len(), 28);

        insta::assert_snapshot!("dimensioned_shapes_0_0_0", mlt_dump(&direct));
        assert_eq!(
            mlt_dump_ignoring_ring_start(&direct),
            mlt_dump_ignoring_ring_start(&round_trip)
        );
    }

    /// Tests that need a `martin` built with `unstable-mlt-v2`.
    #[cfg(feature = "test-mlt-v2")]
    mod mlt_v2 {
        use martin_e2e_tests::{mlt_dump, mlt_dump_ignoring_ring_start};
        use mlt_core::MValue;

        use super::copy_dimensioned;

        /// A v2 tile keeps the M ordinates of every geometry type in the `m` vertex column,
        /// polygons included, while Z is dropped and x and y stay those of the v1 tile.
        #[tokio::test]
        async fn copies_the_m_ordinates_of_every_geometry_type() {
            let v2 = copy_dimensioned("mltv2").await;
            let layer = &v2[0];
            assert_eq!(layer.m_value_names(), ["m"]);
            let dims = layer
                .property_names()
                .iter()
                .position(|name| name == "dims")
                .expect("the layer has no dims column");
            for feature in layer.features() {
                let measured = matches!(
                    &feature.properties()[dims],
                    mlt_core::PropValue::Str(Some(d)) if d.ends_with('m')
                );
                let MValue::F64(m) = &feature.m_values()[0] else {
                    panic!("the m column is not f64: {:?}", feature.m_values());
                };
                assert_eq!(
                    m.as_ref().map(Vec::len),
                    measured.then(|| feature.vertex_count()),
                    "feature {:?} has the wrong M ordinates: {m:?}",
                    feature.id()
                );
            }

            insta::assert_snapshot!("dimensioned_shapes_mltv2_0_0_0", mlt_dump(&v2));
            let without_m = mlt_dump_ignoring_ring_start(&v2)
                .lines()
                .map(|line| line.split(" vertex=").next().unwrap_or(line))
                .collect::<Vec<_>>()
                .join("\n");
            assert_eq!(
                without_m + "\n",
                mlt_dump_ignoring_ring_start(&copy_dimensioned("mlt").await)
            );
        }
    }

    /// The config for the array-column table, whose `int4[]` the row path cannot encode.
    const ARRAYS: &str = "
postgres:
  connection_string: ${DATABASE_URL}
  pool_size: 2
  auto_publish: false
  tables:
    array_props:
      schema: public
      table: array_props
      srid: 4326
      geometry_column: geom
      id_column: feat_id
      bounds: [-180.0, -90.0, 180.0, 90.0]
      properties:
        tags: int4
        label: text
";

    /// An array column reaches the decoder as `_int4`, which it cannot encode. Copying must fall
    /// back to the MVT round-trip rather than abort, since `ST_AsMVT` handles the column fine.
    #[tokio::test]
    async fn copies_a_table_with_an_unencodable_column_through_mvt() {
        let dir = temp_dir();
        let config = dir.path().join("config.yaml");
        fs::write(&config, ARRAYS).expect("failed to write the config");
        let output = dir.path().join("arrays.mbtiles");

        let log = MartinCp::new()
            .with_postgres()
            .env("RUST_LOG", "martin=warn")
            .arg("--config")
            .arg(&config)
            .arg("--set-meta")
            .arg(GENERATOR)
            .arg("--source")
            .arg("array_props")
            .arg("--format")
            .arg("mlt")
            .arg("--encoding")
            .arg("identity")
            .arg("--output-file")
            .arg(&output)
            .arg("--mbtiles-type")
            .arg("flat")
            .arg("--min-zoom")
            .arg("0")
            .arg("--max-zoom")
            .arg("0")
            .run()
            .await;
        assert!(
            log.contains("Copying array_props through MVT"),
            "the copy did not report falling back off the row-per-feature path:\n{log}"
        );

        let metadata = metadata_listing(&output).await;
        insta::assert_snapshot!(
            "array_props_0_0_0",
            mlt_dump(&mlt_layers(&lowest_zoom_tile(&output).await))
        );
        insta::with_settings!({filters => snapshot_filters()}, {
            insta::assert_snapshot!("array_props_metadata", metadata);
        });
    }

    /// The config for an unclipped table, whose tile coordinates leave the `i32` tile space at
    /// the zoom the test copies.
    const UNCLIPPED: &str = "
postgres:
  connection_string: ${DATABASE_URL}
  pool_size: 2
  auto_publish: false
  tables:
    table_source:
      schema: public
      table: table_source
      srid: 4326
      geometry_column: geom
      minzoom: 0
      maxzoom: 30
      clip_geom: false
      bounds: [-180.0, -90.0, 180.0, 90.0]
      properties:
        gid: int4
";

    /// `clip_geom: false` lets `ST_AsMVTGeom` hand out coordinates wider than an `i32`, which the
    /// row path cannot read. Copying must fall back to the MVT round-trip rather than abort,
    /// since `ST_AsMVT` encodes those tiles.
    #[tokio::test]
    async fn copies_a_table_with_unreadable_tile_geometry_through_mvt() {
        let dir = temp_dir();
        let config = dir.path().join("config.yaml");
        fs::write(&config, UNCLIPPED).expect("failed to write the config");
        let output = dir.path().join("unclipped.mbtiles");

        let log = MartinCp::new()
            .with_postgres()
            .env("RUST_LOG", "martin=warn")
            .arg("--config")
            .arg(&config)
            .arg("--set-meta")
            .arg(GENERATOR)
            .arg("--source")
            .arg("table_source")
            .arg("--format")
            .arg("mlt")
            .arg("--encoding")
            .arg("identity")
            .arg("--output-file")
            .arg(&output)
            .arg("--mbtiles-type")
            .arg("flat")
            .arg("--min-zoom")
            .arg("23")
            .arg("--max-zoom")
            .arg("23")
            .arg("--bbox=30.0,10.0,30.00001,10.00001")
            .run()
            .await;
        assert!(
            log.contains("Copying table_source through MVT"),
            "the copy did not report falling back off the row-per-feature path:\n{log}"
        );

        assert!(
            !mlt_layers(&lowest_zoom_tile(&output).await).is_empty(),
            "the copy wrote no MLT layers"
        );
        validate(&output).await;
    }

    /// A function source hands out an MVT blob and has no row form, so `--format mlt` keeps
    /// converting that blob rather than taking the direct path.
    #[tokio::test]
    async fn copies_a_function_source_as_mlt_through_mvt() {
        let dir = temp_dir();
        let output = dir.path().join("out.mbtiles");

        let log = copy(&output)
            .env("RUST_LOG", "martin_core::tiles::postgres=debug")
            .arg("--source")
            .arg("function_zxy_query_test")
            .arg("--url-query")
            .arg("foo=bar&token=martin")
            .arg("--format")
            .arg("mlt")
            .arg("--encoding")
            .arg("identity")
            .arg("--mbtiles-type")
            .arg("flat")
            .arg("--min-zoom")
            .arg("0")
            .arg("--max-zoom")
            .arg("0")
            .run()
            .await;
        assert!(
            !log.contains("ST_AsBinary("),
            "a function source has no row-per-feature query to run:\n{log}"
        );

        let metadata = metadata_listing(&output).await;
        insta::assert_snapshot!(
            "function_source_mlt_0_0_0",
            mlt_dump(&mlt_layers(&lowest_zoom_tile(&output).await))
        );
        insta::with_settings!({filters => snapshot_filters()}, {
            insta::assert_snapshot!("function_source_mlt_metadata", metadata);
        });
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
        let described: String = saved
            .split_inclusive('\n')
            .skip_while(|line| !line.starts_with("    table_source:"))
            .take_while(|line| line.starts_with("    table_source:") || line.starts_with("      "))
            .collect();
        insta::with_settings!({filters => snapshot_filters()}, {
            insta::assert_snapshot!(described.trim_end(), @r"
            table_source:
              schema: public
              table: table_source
              srid: 4326
              geometry_column: geom
              bounds:
              - -2.0
              - -1.0
              - 142.8413150986
              - 45.0
              geometry_type: GEOMETRY
              properties:
                gid: int4
            ");
        });
    }
}

/// A source on another grid is copied on that grid, the whole grid when no bbox is given.
#[cfg(feature = "test-pg")]
#[tokio::test]
async fn copies_a_source_on_another_tile_grid() {
    let dir = temp_dir();
    let config = dir.path().join("config.yaml");
    fs::write(
        &config,
        "
postgres:
  connection_string: ${DATABASE_URL}
  pool_size: 1
  tables:
    nz_points:
      schema: public
      table: nz_points
      srid: 2193
      geometry_column: geom
      tile_grid: NZTM2000Quad
      properties:
        city: text
",
    )
    .expect("write config");
    let output = dir.path().join("nz.mbtiles");

    MartinCp::new()
        .with_postgres()
        .arg("--config")
        .arg(&config)
        .arg("--source")
        .arg("nz_points")
        .arg("--output-file")
        .arg(&output)
        .arg("--mbtiles-type")
        .arg("flat")
        .arg("--min-zoom")
        .arg("0")
        .arg("--max-zoom")
        .arg("2")
        .run()
        .await;

    let summary = summary(&output).run_json().await;
    let metadata = metadata_listing(&output).await;
    insta::with_settings!({filters => summary_filters()}, {
        insta::assert_json_snapshot!("nztm2000quad_copy_summary", summary);
    });
    insta::assert_snapshot!("nztm2000quad_copy_tiles", tile_listing(&output).await);
    insta::with_settings!({filters => snapshot_filters()}, {
        insta::assert_snapshot!("nztm2000quad_copy_metadata", metadata);
    });
    validate(&output).await;
}
