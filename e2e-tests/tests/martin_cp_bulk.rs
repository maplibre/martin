//! `martin cp --mode bulk`, which renders whole tables in one pass.
#![cfg(all(feature = "test-pg", feature = "test-generate"))]

use std::fs;

use indoc::indoc;
use martin_e2e_tests::{
    Martin, MartinCp, MbtilesCli, assert_pmtiles_matches_mbtiles, metadata_listing, mlt_dump, mlt_layers, temp_dir,
    tile_listing, tiles,
};

#[tokio::test]
async fn generates_a_valid_tileset() {
    let dir = temp_dir();
    let config = dir.path().join("config.yaml");
    fs::write(
        &config,
        indoc! {"
            postgres:
              connection_string: ${DATABASE_URL}
              auto_publish: false
              tables:
                points1:
                  schema: public
                  table: points1
                  srid: 4326
                  geometry_column: geom
                  geometry_type: POINT
                  properties:
                    gid: int4
                table_source:
                  schema: public
                  table: table_source
                  filter: \"gid <> 14\"
                  srid: 4326
                  geometry_column: geom
                  geometry_type: GEOMETRY
                  properties:
                    gid: int4
                  layers:
                    mixed: {}
        "},
    )
    .expect("failed to write the config");
    let output = dir.path().join("generated.mbtiles");
    MartinCp::bulk()
        .with_postgres()
        .arg("--config")
        .arg(&config)
        .arg("--output-file")
        .arg(&output)
        .arg("--max-zoom")
        .arg("3")
        .arg("--encoding")
        .arg("none")
        .arg("--threads")
        .arg("2")
        .run()
        .await;
    MbtilesCli::new("validate").arg(&output).run().await;
    insta::assert_snapshot!("tiles", tile_listing(&output).await);
    let metadata = metadata_listing(&output).await;
    insta::with_settings!({ filters => vec![(r"martin cp v[0-9.]+[^\s]*", "martin cp v[VERSION]")] }, {
        insta::assert_snapshot!("metadata", metadata);
    });

    // The output does not depend on the thread count or memory.
    let again = dir.path().join("again.mbtiles");
    MartinCp::bulk()
        .with_postgres()
        .arg("--config")
        .arg(&config)
        .arg("--output-file")
        .arg(&again)
        .arg("--max-zoom")
        .arg("3")
        .arg("--encoding")
        .arg("none")
        .arg("--threads")
        .arg("5")
        .arg("--memory")
        .arg("64MiB")
        .run()
        .await;
    assert_eq!(tiles(&again).await, tiles(&output).await);
}

#[tokio::test]
async fn generates_pmtiles_with_the_same_tiles() {
    let dir = temp_dir();
    let config = dir.path().join("config.yaml");
    fs::write(
        &config,
        indoc! {"
            postgres:
              connection_string: ${DATABASE_URL}
              auto_publish: false
              tables:
                points1:
                  schema: public
                  table: points1
                  srid: 4326
                  geometry_column: geom
                  geometry_type: POINT
                  properties:
                    gid: int4
                table_source:
                  schema: public
                  table: table_source
                  filter: \"gid <> 14\"
                  srid: 4326
                  geometry_column: geom
                  geometry_type: GEOMETRY
                  properties:
                    gid: int4
                  layers:
                    mixed: {}
        "},
    )
    .expect("failed to write the config");
    let (mbtiles, pmtiles) = (dir.path().join("out.mbtiles"), dir.path().join("out.pmtiles"));
    MartinCp::bulk()
        .with_postgres()
        .arg("--config")
        .arg(&config)
        .arg("--output-file")
        .arg(&mbtiles)
        .arg("--max-zoom")
        .arg("3")
        .arg("--encoding")
        .arg("none")
        .run()
        .await;
    MartinCp::bulk()
        .with_postgres()
        .arg("--config")
        .arg(&config)
        .arg("--output-file")
        .arg(&pmtiles)
        .arg("--max-zoom")
        .arg("3")
        .arg("--encoding")
        .arg("none")
        .run()
        .await;
    assert_pmtiles_matches_mbtiles(&pmtiles, &mbtiles).await;

    // An existing archive is neither overwritten nor removed.
    let log = MartinCp::bulk()
        .with_postgres()
        .arg("--config")
        .arg(&config)
        .arg("--output-file")
        .arg(&pmtiles)
        .arg("--max-zoom")
        .arg("3")
        .arg("--encoding")
        .arg("none")
        .run_expecting_failure()
        .await;
    assert!(log.contains("not empty"), "{log}");
    assert_pmtiles_matches_mbtiles(&pmtiles, &mbtiles).await;
}

#[tokio::test]
async fn martin_serves_the_default_pmtiles_output() {
    let dir = temp_dir();
    let pmtiles = dir.path().join("out.pmtiles");
    MartinCp::bulk()
        .with_postgres()
        .arg("--source")
        .arg("points1")
        .arg("--output-file")
        .arg(&pmtiles)
        .arg("--max-zoom")
        .arg("1")
        .run()
        .await;
    let mut martin = Martin::builder()
        .arg(&pmtiles)
        .start()
        .await
        .expect("failed to start martin");
    let tile = martin.get("/out/0/0/0").await;
    insta::assert_snapshot!(tile.headers_snapshot_masking_etag(), @"
    cache-control: max-age=0, stale-while-revalidate=86400
    content-length: 193
    content-type: application/vnd.maplibre-tile
    etag: [ETAG]
    vary: accept-encoding, Origin, Access-Control-Request-Method, Access-Control-Request-Headers
    ");
    insta::assert_snapshot!(mlt_dump(&tile.mlt()), @"
    layer points1 extent 4096
      id=None geom=MULTIPOINT(4104 1845,8 1845) props=[gid=U32(Some(9))]
      id=None geom=MULTIPOINT(4145 2032,49 2032) props=[gid=U32(Some(21))]
      id=None geom=POINT(1310 3101) props=[gid=U32(Some(26))]
      id=None geom=POINT(1377 2078) props=[gid=U32(Some(27))]
      id=None geom=POINT(1415 16) props=[gid=U32(Some(16))]
      id=None geom=POINT(1455 2502) props=[gid=U32(Some(14))]
      id=None geom=POINT(166 2707) props=[gid=U32(Some(28))]
      id=None geom=POINT(1728 2658) props=[gid=U32(Some(25))]
      id=None geom=POINT(2312 2004) props=[gid=U32(Some(12))]
      id=None geom=POINT(2316 1790) props=[gid=U32(Some(24))]
      id=None geom=POINT(2337 2613) props=[gid=U32(Some(13))]
      id=None geom=POINT(2583 2400) props=[gid=U32(Some(23))]
      id=None geom=POINT(2619 1520) props=[gid=U32(Some(8))]
      id=None geom=POINT(2749 1706) props=[gid=U32(Some(20))]
      id=None geom=POINT(2754 966) props=[gid=U32(Some(17))]
      id=None geom=POINT(2780 1898) props=[gid=U32(Some(30))]
      id=None geom=POINT(2961 2263) props=[gid=U32(Some(11))]
      id=None geom=POINT(3553 2982) props=[gid=U32(Some(22))]
      id=None geom=POINT(3639 2944) props=[gid=U32(Some(10))]
      id=None geom=POINT(3673 1911) props=[gid=U32(Some(1))]
      id=None geom=POINT(3673 1911) props=[gid=U32(Some(2))]
      id=None geom=POINT(3673 1911) props=[gid=U32(Some(3))]
      id=None geom=POINT(3786 1562) props=[gid=U32(Some(6))]
      id=None geom=POINT(3802 665) props=[gid=U32(Some(7))]
      id=None geom=POINT(3825 2146) props=[gid=U32(Some(19))]
      id=None geom=POINT(3898 1322) props=[gid=U32(Some(29))]
      id=None geom=POINT(636 2785) props=[gid=U32(Some(4))]
      id=None geom=POINT(69 313) props=[gid=U32(Some(5))]
      id=None geom=POINT(769 2818) props=[gid=U32(Some(18))]
      id=None geom=POINT(862 1817) props=[gid=U32(Some(15))]
    ");
    martin.stop().await;
}

#[tokio::test]
async fn spreads_jsonb_keys_over_properties_as_martin_serves_them() {
    let config_yaml = indoc! {"
        postgres:
          connection_string: ${DATABASE_URL}
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
                doc: jsonb
    "};
    let dir = temp_dir();
    let config = dir.path().join("config.yaml");
    fs::write(&config, config_yaml).expect("failed to write the config");
    let output = dir.path().join("out.mbtiles");
    MartinCp::bulk()
        .with_postgres()
        .arg("--config")
        .arg(&config)
        .arg("--output-file")
        .arg(&output)
        .arg("--max-zoom")
        .arg("0")
        .arg("--encoding")
        .arg("none")
        .run()
        .await;
    let (_, _, _, data) = tiles(&output)
        .await
        .into_iter()
        .next()
        .expect("the copy wrote no tiles");
    let generated = mlt_layers(&data);

    let mut martin = Martin::builder()
        .with_postgres()
        .config(config_yaml)
        .start()
        .await
        .expect("failed to start martin");
    let served = martin
        .get_with_headers("/array_props/0/0/0", &[("Accept", "application/vnd.maplibre-tile")])
        .await
        .mlt();
    martin.stop().await;

    insta::assert_snapshot!(mlt_dump(&generated), @r#"
    layer array_props extent 4096
      id=Some(1) geom=POINT(2048 2048) props=[kind=Str(Some("a")), label=Str(Some("one")), rank=I32(Some(1)), score=F64(Some(1.5)), tags=Str(Some("{1,2,3}"))]
      id=Some(2) geom=POINT(2059 2037) props=[kind=Str(None), label=Str(Some("two")), rank=I32(Some(-2)), score=F64(Some(3.0)), tags=Str(None)]
    "#);
    assert_eq!(mlt_dump(&generated), mlt_dump(&served));
}

#[tokio::test]
#[expect(clippy::too_many_lines, reason = "inline snapshot of the tile")]
async fn martin_serves_mvt_output_in_every_compression() {
    let dir = temp_dir();
    let mut dumps = Vec::new();
    for (output, encoding) in [("gzip.mbtiles", "gzip"), ("br.pmtiles", "br"), ("zstd.pmtiles", "zstd")] {
        let output = dir.path().join(output);
        MartinCp::bulk()
            .with_postgres()
            .arg("--source")
            .arg("points1")
            .arg("--output-file")
            .arg(&output)
            .arg("--format")
            .arg("mvt")
            .arg("--encoding")
            .arg(encoding)
            .arg("--max-zoom")
            .arg("1")
            .run()
            .await;
        let mut martin = Martin::builder()
            .arg(&output)
            .start()
            .await
            .expect("failed to start martin");
        let name = output
            .file_stem()
            .and_then(|stem| stem.to_str())
            .expect("a utf-8 file name");
        let tile = martin
            .get_with_headers(&format!("/{name}/0/0/0"), &[("Accept-Encoding", encoding)])
            .await;
        martin.stop().await;
        assert_eq!(tile.header("content-encoding"), Some(encoding), "{name}");
        assert_eq!(tile.header("content-type"), Some("application/x-protobuf"), "{name}");
        dumps.push(tile.mvt_dump());
    }
    insta::assert_snapshot!(dumps[0], @"
    layer: 0
      name: points1
      version: 2
      extent: 4096
      feature: 0
        id: (none)
        geometry: POINT(2312,2004)
        properties:
          gid = 12 (uint)
      feature: 1
        id: (none)
        geometry: POINT(2316,1790)
        properties:
          gid = 24 (uint)
      feature: 2
        id: (none)
        geometry: POINT(2619,1520)
        properties:
          gid = 8 (uint)
      feature: 3
        id: (none)
        geometry: POINT(2754,966)
        properties:
          gid = 17 (uint)
      feature: 4
        id: (none)
        geometry: POINT(3802,665)
        properties:
          gid = 7 (uint)
      feature: 5
        id: (none)
        geometry: POINT(3898,1322)
        properties:
          gid = 29 (uint)
      feature: 6
        id: (none)
        geometry: POINT(3786,1562)
        properties:
          gid = 6 (uint)
      feature: 7
        id: (none)
        geometry: POINT(3673,1911)
        properties:
          gid = 3 (uint)
      feature: 8
        id: (none)
        geometry: POINT(3673,1911)
        properties:
          gid = 2 (uint)
      feature: 9
        id: (none)
        geometry: POINT(3673,1911)
        properties:
          gid = 1 (uint)
      feature: 10
        id: (none)
        geometry: POINT(2749,1706)
        properties:
          gid = 20 (uint)
      feature: 11
        id: (none)
        geometry: POINT(2780,1898)
        properties:
          gid = 30 (uint)
      feature: 12
        id: (none)
        geometry:
          POINT(4145,2032)
          POINT(49,2032)
        properties:
          gid = 21 (uint)
      feature: 13
        id: (none)
        geometry:
          POINT(4104,1845)
          POINT(8,1845)
        properties:
          gid = 9 (uint)
      feature: 14
        id: (none)
        geometry: POINT(862,1817)
        properties:
          gid = 15 (uint)
      feature: 15
        id: (none)
        geometry: POINT(1415,16)
        properties:
          gid = 16 (uint)
      feature: 16
        id: (none)
        geometry: POINT(69,313)
        properties:
          gid = 5 (uint)
      feature: 17
        id: (none)
        geometry: POINT(1377,2078)
        properties:
          gid = 27 (uint)
      feature: 18
        id: (none)
        geometry: POINT(1455,2502)
        properties:
          gid = 14 (uint)
      feature: 19
        id: (none)
        geometry: POINT(1310,3101)
        properties:
          gid = 26 (uint)
      feature: 20
        id: (none)
        geometry: POINT(636,2785)
        properties:
          gid = 4 (uint)
      feature: 21
        id: (none)
        geometry: POINT(769,2818)
        properties:
          gid = 18 (uint)
      feature: 22
        id: (none)
        geometry: POINT(166,2707)
        properties:
          gid = 28 (uint)
      feature: 23
        id: (none)
        geometry: POINT(1728,2658)
        properties:
          gid = 25 (uint)
      feature: 24
        id: (none)
        geometry: POINT(3639,2944)
        properties:
          gid = 10 (uint)
      feature: 25
        id: (none)
        geometry: POINT(3553,2982)
        properties:
          gid = 22 (uint)
      feature: 26
        id: (none)
        geometry: POINT(2337,2613)
        properties:
          gid = 13 (uint)
      feature: 27
        id: (none)
        geometry: POINT(2583,2400)
        properties:
          gid = 23 (uint)
      feature: 28
        id: (none)
        geometry: POINT(2961,2263)
        properties:
          gid = 11 (uint)
      feature: 29
        id: (none)
        geometry: POINT(3825,2146)
        properties:
          gid = 19 (uint)
    ");
    assert_eq!(dumps[1], dumps[0]);
    assert_eq!(dumps[2], dumps[0]);
}

#[tokio::test]
async fn refuses_encodings_martin_cannot_serve() {
    let dir = temp_dir();
    let mut log = String::new();
    for (output, encoding) in [
        ("out.pmtiles", "gzip"),
        ("out.pmtiles", "br"),
        ("out.mbtiles", "br"),
        ("out.mbtiles", "zstd"),
    ] {
        log += &MartinCp::bulk()
            .env("RUST_LOG", "error")
            .with_postgres()
            .arg("--output-file")
            .arg(dir.path().join(output))
            .arg("--encoding")
            .arg(encoding)
            .arg("--max-zoom")
            .arg("0")
            .run_expecting_failure()
            .await;
    }
    insta::assert_snapshot!(log, @"
    ERROR Martin cannot serve compressed MLT tiles from PMTiles; use `--encoding none`
    ERROR Martin cannot serve compressed MLT tiles from PMTiles; use `--encoding none`
    ERROR Martin cannot detect br compression in MBTiles; use `--encoding gzip`, `zlib` or `none`
    ERROR Martin cannot detect zstd compression in MBTiles; use `--encoding gzip`, `zlib` or `none`
    ");
}

#[tokio::test]
async fn refuses_options_only_tile_mode_reads() {
    let dir = temp_dir();
    let log = MartinCp::bulk()
        .with_postgres()
        .arg("--output-file")
        .arg(dir.path().join("out.mbtiles"))
        .arg("--url-query")
        .arg("a=1")
        .arg("--mbtiles-type")
        .arg("flat")
        .arg("--on-duplicate")
        .arg("override")
        .arg("--concurrency")
        .arg("4")
        .arg("--skip-agg-tiles-hash")
        .arg("--set-meta")
        .arg("name=x")
        .arg("--delete-meta")
        .arg("bounds")
        .arg("--save-config")
        .arg("saved.yaml")
        .arg("--max-feature-count")
        .arg("10")
        .run_expecting_failure()
        .await;
    insta::assert_snapshot!(log, @"ERROR `--mode bulk` does not read `--url-query`, `--mbtiles-type`, `--on-duplicate`, `--concurrency`, `--skip-agg-tiles-hash`, `--set-meta`, `--delete-meta`, `--save-config`, `--max-feature-count`");
}

#[tokio::test]
async fn refuses_arguments_bulk_mode_cannot_honour() {
    let dir = temp_dir();
    let mut log = String::new();
    for args in [
        &["--bbox=-10,-10,0,0", "--bbox=0,0,10,10"][..],
        &["--zoom-levels", "0,2"],
        &["--encoding", "gzip,br"],
    ] {
        let mut cp = MartinCp::bulk()
            .with_postgres()
            .arg("--output-file")
            .arg(dir.path().join("out.mbtiles"));
        for arg in args {
            cp = cp.arg(arg);
        }
        log += &cp.run_expecting_failure().await;
    }
    insta::assert_snapshot!(log, @"
    ERROR `--mode bulk` takes at most one `--bbox`
    ERROR `--mode bulk` renders a range of zooms; `--zoom-levels` must not skip a zoom
    ERROR unknown encoding `gzip,br`; `--mode bulk` takes one of gzip, zlib, br, zstd or none
    ");
}

#[cfg(feature = "test-mlt-v2")]
#[tokio::test]
async fn refuses_mlt2() {
    let dir = temp_dir();
    let log = MartinCp::bulk()
        .with_postgres()
        .arg("--output-file")
        .arg(dir.path().join("out.mbtiles"))
        .arg("--format")
        .arg("mlt2")
        .run_expecting_failure()
        .await;
    insta::assert_snapshot!(log, @"ERROR `--mode bulk` cannot write `--format mlt2` yet");
}

#[tokio::test]
async fn contiguous_zoom_levels_are_a_zoom_range() {
    let dir = temp_dir();
    let (levels, range) = (dir.path().join("levels.mbtiles"), dir.path().join("range.mbtiles"));
    MartinCp::bulk()
        .with_postgres()
        .arg("--source")
        .arg("points1,points2")
        .arg("--output-file")
        .arg(&levels)
        .arg("--zoom-levels")
        .arg("2,1,3")
        .run()
        .await;
    MartinCp::bulk()
        .with_postgres()
        .arg("--source")
        .arg("points1,points2")
        .arg("--output-file")
        .arg(&range)
        .arg("--min-zoom")
        .arg("1")
        .arg("--max-zoom")
        .arg("3")
        .run()
        .await;
    assert_eq!(tiles(&levels).await, tiles(&range).await);
    let listing = tile_listing(&levels).await;
    let mut zooms: Vec<&str> = listing.lines().filter_map(|tile| tile.split('/').next()).collect();
    zooms.dedup();
    insta::assert_snapshot!(zooms.join(","), @"1,2,3");
}

#[tokio::test]
async fn one_table_feeds_several_layers() {
    let dir = temp_dir();
    let config = dir.path().join("config.yaml");
    fs::write(
        &config,
        indoc! {"
            postgres:
              connection_string: ${DATABASE_URL}
              auto_publish: false
              tables:
                table_source:
                  schema: public
                  table: table_source
                  filter: \"gid <> 14\"
                  srid: 4326
                  geometry_column: geom
                  geometry_type: GEOMETRY
                  id_column: gid
                  properties:
                    gid: int4
                  layers:
                    shapes:
                      maxzoom: 1
                      attributes: []
                      id: false
                    points:
                      minzoom: 1
                      geometry: point
                    lines:
                      geometry: line
                      buffer: 0
                      simplify: 0
                    detail:
                      minzoom: 10
        "},
    )
    .expect("failed to write the config");
    let output = dir.path().join("layers.mbtiles");
    let log = MartinCp::bulk()
        .with_postgres()
        .arg("--config")
        .arg(&config)
        .arg("--output-file")
        .arg(&output)
        .arg("--max-zoom")
        .arg("3")
        .arg("--encoding")
        .arg("none")
        .run()
        .await;
    assert!(
        log.contains("Skipping layer `detail` of source `table_source`"),
        "{log}"
    );
    MbtilesCli::new("validate").arg(&output).run().await;
    insta::assert_snapshot!("layers_tiles", tile_listing(&output).await);
    let metadata = metadata_listing(&output).await;
    insta::with_settings!({ filters => vec![(r"martin cp v[0-9.]+[^\s]*", "martin cp v[VERSION]")] }, {
        insta::assert_snapshot!("layers_metadata", metadata);
    });
}

#[tokio::test]
async fn layer_expressions_filter_features_and_compute_attributes() {
    let dir = temp_dir();
    let config = dir.path().join("config.yaml");
    fs::write(
        &config,
        indoc! {"
            postgres:
              connection_string: ${DATABASE_URL}
              auto_publish: false
              tables:
                table_source:
                  schema: public
                  table: table_source
                  filter: \"gid <> 14\"
                  srid: 4326
                  geometry_column: geom
                  geometry_type: GEOMETRY
                  id_column: gid
                  properties:
                    gid: int4
                  layers:
                    even:
                      where: 'gid % 2 == 0'
                      minzoom: 'gid > 8 ? 2 : 0'
                      id: 'gid * 100'
                      attributes:
                        number: gid
                        size: { expr: \"gid > 5 ? 'big' : 'small'\", minzoom: 2 }
                        ratio: '12 / (gid - 4)'
                        source: { value: fixture }
        "},
    )
    .expect("failed to write the config");
    let output = dir.path().join("expressions.mbtiles");
    let log = MartinCp::bulk()
        .with_postgres()
        .arg("--config")
        .arg(&config)
        .arg("--output-file")
        .arg(&output)
        .arg("--max-zoom")
        .arg("3")
        .arg("--encoding")
        .arg("none")
        .run()
        .await;
    assert!(log.contains("Layer `even`: `12 / (gid - 4)` failed"), "{log}");
    MbtilesCli::new("validate").arg(&output).run().await;
    let dump: Vec<String> = tiles(&output)
        .await
        .iter()
        .map(|(z, x, y, data)| format!("{z}/{x}/{y}\n{}", mlt_dump(&mlt_layers(data))))
        .collect();
    insta::assert_snapshot!("expressions_tiles", dump.join("\n"));
    let metadata = metadata_listing(&output).await;
    insta::with_settings!({ filters => vec![(r"martin cp v[0-9.]+[^\s]*", "martin cp v[VERSION]")] }, {
        insta::assert_snapshot!("expressions_metadata", metadata);
    });
}

#[tokio::test]
async fn skips_a_source_with_no_zoom_in_range() {
    let dir = temp_dir();
    let config = dir.path().join("config.yaml");
    fs::write(
        &config,
        indoc! {"
            postgres:
              connection_string: ${DATABASE_URL}
              auto_publish: false
              tables:
                points1:
                  schema: public
                  table: points1
                  srid: 4326
                  geometry_column: geom
                  geometry_type: POINT
                points2:
                  schema: public
                  table: points2
                  srid: 4326
                  geometry_column: geom
                  geometry_type: POINT
                  minzoom: 12
        "},
    )
    .expect("failed to write the config");
    let output = dir.path().join("out.mbtiles");
    let log = MartinCp::bulk()
        .env("RUST_LOG", "warn")
        .with_postgres()
        .arg("--config")
        .arg(&config)
        .arg("--output-file")
        .arg(&output)
        .arg("--max-zoom")
        .arg("1")
        .run()
        .await;
    insta::assert_snapshot!(log, @"
    WARN Unable to deserialize SQL comment as tilejson, the automatically generated tilejson would be used schema=public table=points2 error=expected value at line 1 column 1
    WARN Skipping source `points2`: it has no zoom between 0 and 1
    ");
    let metadata = metadata_listing(&output).await;
    insta::with_settings!({ filters => vec![(r"martin cp v[0-9.]+[^\s]*", "martin cp v[VERSION]")] }, {
        insta::assert_snapshot!(metadata, @r#"
        agg_tiles_hash = E0468BC04F4DE5AE1C2E0CC5B1FE4284
        format = mlt
        generator = martin cp v[VERSION] (bulk)
        hash_algorithm = md5
        json = {
          "vector_layers": [
            {
              "fields": {},
              "id": "points1",
              "maxzoom": 1,
              "minzoom": 0
            }
          ]
        }
        maxzoom = 1
        minzoom = 0
        name = out
        "#);
    });
}

#[tokio::test]
async fn features_whose_zooms_miss_the_generated_zooms_are_dropped() {
    let dir = temp_dir();
    let config = dir.path().join("config.yaml");
    fs::write(
        &config,
        indoc! {"
            postgres:
              connection_string: ${DATABASE_URL}
              auto_publish: false
              tables:
                points1:
                  schema: public
                  table: points1
                  srid: 4326
                  geometry_column: geom
                  geometry_type: POINT
                  id_column: gid
                  properties:
                    gid: int4
                  layers:
                    points:
                      minzoom: 'gid > 25 ? 5 : 0'
                      maxzoom: 'gid < 5 ? 1 : 9'
        "},
    )
    .expect("failed to write the config");
    let output = dir.path().join("zooms.mbtiles");
    MartinCp::bulk()
        .with_postgres()
        .arg("--config")
        .arg(&config)
        .arg("--output-file")
        .arg(&output)
        .arg("--min-zoom")
        .arg("2")
        .arg("--max-zoom")
        .arg("3")
        .arg("--encoding")
        .arg("none")
        .run()
        .await;
    let mut ids = std::collections::BTreeMap::<i64, std::collections::BTreeSet<u64>>::new();
    for (z, _, _, data) in tiles(&output).await {
        for layer in mlt_layers(&data) {
            ids.entry(z)
                .or_default()
                .extend(layer.features().iter().filter_map(mlt_core::TileFeature::id));
        }
    }
    insta::assert_snapshot!(format!("{ids:?}"), @"{2: {5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25}, 3: {5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25}}");
}

#[tokio::test]
async fn layer_rules_override_settings_per_feature() {
    let dir = temp_dir();
    let config = dir.path().join("config.yaml");
    fs::write(
        &config,
        indoc! {"
            postgres:
              connection_string: ${DATABASE_URL}
              auto_publish: false
              tables:
                table_source:
                  schema: public
                  table: table_source
                  filter: \"gid <> 14\"
                  srid: 4326
                  geometry_column: geom
                  geometry_type: GEOMETRY
                  id_column: gid
                  properties:
                    gid: int4
                  layers:
                    ranked:
                      maxzoom: 2
                      attributes:
                        number: gid
                        band: { value: low }
                      rules:
                        - where: 'gid > 15'
                          attributes:
                            band: { value: high }
                            half: { expr: 'gid / 2', minzoom: 2 }
                        - where: 'gid > 5'
                          minzoom: 1
                          simplify: 0
                          attributes:
                            band: { value: mid }
                        - minzoom: 'gid % 2 == 0 ? 1 : 2'
        "},
    )
    .expect("failed to write the config");
    let output = dir.path().join("rules.mbtiles");
    MartinCp::bulk()
        .with_postgres()
        .arg("--config")
        .arg(&config)
        .arg("--output-file")
        .arg(&output)
        .arg("--max-zoom")
        .arg("3")
        .arg("--encoding")
        .arg("none")
        .run()
        .await;
    MbtilesCli::new("validate").arg(&output).run().await;
    let dump: Vec<String> = tiles(&output)
        .await
        .iter()
        .map(|(z, x, y, data)| format!("{z}/{x}/{y}\n{}", mlt_dump(&mlt_layers(data))))
        .collect();
    insta::assert_snapshot!("rules_tiles", dump.join("\n"));
    let metadata = metadata_listing(&output).await;
    insta::with_settings!({ filters => vec![(r"martin cp v[0-9.]+[^\s]*", "martin cp v[VERSION]")] }, {
        insta::assert_snapshot!("rules_metadata", metadata);
    });
}

#[tokio::test]
async fn features_whose_rule_zooms_miss_the_generated_zooms_are_dropped() {
    let dir = temp_dir();
    let config = dir.path().join("config.yaml");
    fs::write(
        &config,
        indoc! {"
            postgres:
              connection_string: ${DATABASE_URL}
              auto_publish: false
              tables:
                points1:
                  schema: public
                  table: points1
                  srid: 4326
                  geometry_column: geom
                  geometry_type: POINT
                  id_column: gid
                  properties:
                    gid: int4
                  layers:
                    points:
                      rules:
                        - where: 'gid > 25'
                          minzoom: 5
                        - where: 'gid < 5'
                          maxzoom: 1
        "},
    )
    .expect("failed to write the config");
    let output = dir.path().join("zooms.mbtiles");
    MartinCp::bulk()
        .with_postgres()
        .arg("--config")
        .arg(&config)
        .arg("--output-file")
        .arg(&output)
        .arg("--min-zoom")
        .arg("2")
        .arg("--max-zoom")
        .arg("3")
        .arg("--encoding")
        .arg("none")
        .run()
        .await;
    let mut ids = std::collections::BTreeMap::<i64, std::collections::BTreeSet<u64>>::new();
    for (z, _, _, data) in tiles(&output).await {
        for layer in mlt_layers(&data) {
            ids.entry(z)
                .or_default()
                .extend(layer.features().iter().filter_map(mlt_core::TileFeature::id));
        }
    }
    insta::assert_snapshot!(format!("{ids:?}"), @"{2: {5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25}, 3: {5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25}}");
}

#[tokio::test]
async fn sort_by_sets_the_draw_order() {
    let dir = temp_dir();
    let config = dir.path().join("config.yaml");
    fs::write(
        &config,
        indoc! {"
            postgres:
              connection_string: ${DATABASE_URL}
              auto_publish: false
              tables:
                table_source:
                  schema: public
                  table: table_source
                  filter: \"gid <> 14\"
                  srid: 4326
                  geometry_column: geom
                  geometry_type: GEOMETRY
                  id_column: gid
                  properties:
                    gid: int4
                  layers:
                    sorted:
                      maxzoom: 1
                      attributes:
                        number: gid
                      sort_by: [{ expr: 'gid % 4', desc: true }, gid]
        "},
    )
    .expect("failed to write the config");
    let output = dir.path().join("sorted.mbtiles");
    MartinCp::bulk()
        .with_postgres()
        .arg("--config")
        .arg(&config)
        .arg("--output-file")
        .arg(&output)
        .arg("--max-zoom")
        .arg("3")
        .arg("--encoding")
        .arg("none")
        .run()
        .await;
    MbtilesCli::new("validate").arg(&output).run().await;
    let order: Vec<String> = tiles(&output)
        .await
        .iter()
        .flat_map(|(z, x, y, data)| {
            mlt_layers(data).into_iter().map(move |layer| {
                let ids: Vec<String> = layer
                    .features()
                    .iter()
                    .map(|feature| feature.id().map_or("-".to_owned(), |id| id.to_string()))
                    .collect();
                format!("{z}/{x}/{y} {} {}", layer.name(), ids.join(" "))
            })
        })
        .collect();
    insta::assert_snapshot!("sort_by_order", order.join("\n"));
    let metadata = metadata_listing(&output).await;
    insta::with_settings!({ filters => vec![(r"martin cp v[0-9.]+[^\s]*", "martin cp v[VERSION]")] }, {
        insta::assert_snapshot!("sort_by_metadata", metadata);
    });
}

#[tokio::test]
async fn encodes_with_the_configured_convert_to_mlt_settings() {
    let dir = temp_dir();
    let default_config = dir.path().join("default.yaml");
    fs::write(
        &default_config,
        indoc! {"
            postgres:
              connection_string: ${DATABASE_URL}
              auto_publish: false
              tables:
                table_source:
                  schema: public
                  table: table_source
                  filter: \"gid <> 14\"
                  srid: 4326
                  geometry_column: geom
                  geometry_type: GEOMETRY
                  properties:
                    gid: int4
        "},
    )
    .expect("failed to write the config");
    let tessellated_config = dir.path().join("tessellated.yaml");
    fs::write(
        &tessellated_config,
        indoc! {"
            convert_to_mlt:
              tessellate: true
            postgres:
              connection_string: ${DATABASE_URL}
              auto_publish: false
              tables:
                table_source:
                  schema: public
                  table: table_source
                  filter: \"gid <> 14\"
                  srid: 4326
                  geometry_column: geom
                  geometry_type: GEOMETRY
                  properties:
                    gid: int4
        "},
    )
    .expect("failed to write the config");
    let default_output = dir.path().join("default.mbtiles");
    MartinCp::bulk()
        .with_postgres()
        .arg("--config")
        .arg(&default_config)
        .arg("--output-file")
        .arg(&default_output)
        .arg("--max-zoom")
        .arg("1")
        .arg("--encoding")
        .arg("none")
        .run()
        .await;
    let tessellated_output = dir.path().join("tessellated.mbtiles");
    MartinCp::bulk()
        .with_postgres()
        .arg("--config")
        .arg(&tessellated_config)
        .arg("--output-file")
        .arg(&tessellated_output)
        .arg("--max-zoom")
        .arg("1")
        .arg("--encoding")
        .arg("none")
        .run()
        .await;
    let listings = format!(
        "default:\n{}\ntessellated:\n{}",
        tile_listing(&default_output).await,
        tile_listing(&tessellated_output).await
    );
    insta::assert_snapshot!(listings, @"
    default:
    0/0/0 412 bytes
    1/0/0 190 bytes
    1/0/1 212 bytes
    1/1/0 206 bytes
    1/1/1 478 bytes
    tessellated:
    0/0/0 548 bytes
    1/0/0 280 bytes
    1/0/1 302 bytes
    1/1/0 296 bytes
    1/1/1 637 bytes
    ");
}

#[tokio::test]
async fn rejects_function_sources() {
    let dir = temp_dir();
    let log = MartinCp::bulk()
        .with_postgres()
        .arg("--output-file")
        .arg(dir.path().join("out.mbtiles"))
        .arg("--source")
        .arg("function_zxy_query")
        .run_expecting_failure()
        .await;
    assert!(log.contains("function source"), "{log}");
}

#[tokio::test]
async fn refuses_an_unknown_source() {
    let dir = temp_dir();
    let config = dir.path().join("config.yaml");
    fs::write(
        &config,
        indoc! {"
            postgres:
              connection_string: ${DATABASE_URL}
              auto_publish: false
              tables:
                points1:
                  schema: public
                  table: points1
                  srid: 4326
                  geometry_column: geom
                  geometry_type: POINT
        "},
    )
    .expect("failed to write the config");
    let output = dir.path().join("out.mbtiles");
    let log = MartinCp::bulk()
        .env("RUST_LOG", "error")
        .with_postgres()
        .arg("--config")
        .arg(&config)
        .arg("--output-file")
        .arg(&output)
        .arg("--source")
        .arg("points9")
        .run_expecting_failure()
        .await;
    insta::assert_snapshot!(log, @"ERROR No table source matches `points9`. Available table sources: points1");
    assert!(!output.exists());
}

#[tokio::test]
async fn refuses_a_source_on_another_tile_grid() {
    let dir = temp_dir();
    let config = dir.path().join("config.yaml");
    fs::write(
        &config,
        indoc! {"
            tile_grids:
              WGS84Square:
                crs: EPSG:4326
                origin: [-180, 90]
                extent_at_zoom0: 360
            postgres:
              connection_string: ${DATABASE_URL}
              auto_publish: false
              tables:
                points1:
                  schema: public
                  table: points1
                  srid: 4326
                  geometry_column: geom
                  geometry_type: POINT
                  tile_grid: WGS84Square
        "},
    )
    .expect("failed to write the config");
    let output = dir.path().join("out.mbtiles");
    let log = MartinCp::bulk()
        .env("RUST_LOG", "error")
        .with_postgres()
        .arg("--config")
        .arg(&config)
        .arg("--output-file")
        .arg(&output)
        .run_expecting_failure()
        .await;
    insta::assert_snapshot!(log, @"ERROR Source `points1` uses tile grid `WGS84Square`; `--mode bulk` supports Web Mercator only");
    assert!(!output.exists());
}

#[tokio::test]
async fn refuses_sources_from_two_databases() {
    let dir = temp_dir();
    let config = dir.path().join("config.yaml");
    fs::write(
        &config,
        indoc! {"
            postgres:
              - connection_string: ${DATABASE_URL}
                auto_publish: false
                tables:
                  points1:
                    schema: public
                    table: points1
                    srid: 4326
                    geometry_column: geom
                    geometry_type: POINT
              - connection_string: ${DATABASE_URL}
                auto_publish: false
                tables:
                  points2:
                    schema: public
                    table: points2
                    srid: 4326
                    geometry_column: geom
                    geometry_type: POINT
        "},
    )
    .expect("failed to write the config");
    let output = dir.path().join("out.mbtiles");
    let log = MartinCp::bulk()
        .env("RUST_LOG", "error")
        .with_postgres()
        .arg("--config")
        .arg(&config)
        .arg("--output-file")
        .arg(&output)
        .run_expecting_failure()
        .await;
    insta::assert_snapshot!(log, @"ERROR Sources come from more than one database; generate them separately");
    assert!(!output.exists());
}

#[tokio::test]
async fn refuses_zooms_no_layer_has() {
    let dir = temp_dir();
    let config = dir.path().join("config.yaml");
    fs::write(
        &config,
        indoc! {"
            postgres:
              connection_string: ${DATABASE_URL}
              auto_publish: false
              tables:
                points1:
                  schema: public
                  table: points1
                  srid: 4326
                  geometry_column: geom
                  geometry_type: POINT
                  maxzoom: 2
        "},
    )
    .expect("failed to write the config");
    let output = dir.path().join("out.mbtiles");
    let log = MartinCp::bulk()
        .env("RUST_LOG", "error")
        .with_postgres()
        .arg("--config")
        .arg(&config)
        .arg("--output-file")
        .arg(&output)
        .arg("--min-zoom")
        .arg("5")
        .arg("--max-zoom")
        .arg("6")
        .run_expecting_failure()
        .await;
    insta::assert_snapshot!(log, @"ERROR No layer has a zoom between 5 and 6");
    assert!(!output.exists());
}

#[tokio::test]
async fn bbox_tiles_keep_every_feature_they_cover() {
    let dir = temp_dir();
    let config = dir.path().join("config.yaml");
    fs::write(
        &config,
        indoc! {"
            postgres:
              connection_string: ${DATABASE_URL}
              auto_publish: false
              tables:
                points1:
                  schema: public
                  table: points1
                  srid: 4326
                  geometry_column: geom
                  geometry_type: POINT
                  properties:
                    gid: int4
        "},
    )
    .expect("failed to write the config");
    let bounded = dir.path().join("bounded.mbtiles");
    let world = dir.path().join("world.mbtiles");
    MartinCp::bulk()
        .with_postgres()
        .arg("--config")
        .arg(&config)
        .arg("--output-file")
        .arg(&bounded)
        .arg("--bbox=20,1,30,10")
        .arg("--max-zoom")
        .arg("3")
        .arg("--encoding")
        .arg("none")
        .run()
        .await;
    MartinCp::bulk()
        .with_postgres()
        .arg("--config")
        .arg(&config)
        .arg("--output-file")
        .arg(&world)
        .arg("--max-zoom")
        .arg("3")
        .arg("--encoding")
        .arg("none")
        .run()
        .await;
    insta::assert_snapshot!(tile_listing(&bounded).await, @"
    0/0/0 193 bytes
    1/1/1 95 bytes
    2/2/2 60 bytes
    3/4/4 43 bytes
    ");
    let world = tiles(&world).await;
    for tile in tiles(&bounded).await {
        assert!(
            world.contains(&tile),
            "tile {}/{}/{} differs from the unbounded run",
            tile.0,
            tile.1,
            tile.2
        );
    }
}

#[tokio::test]
async fn rejects_inverted_and_off_world_bboxes() {
    let dir = temp_dir();
    for (bbox, expected) in [
        ("--bbox=170,-50,-170,-30", "has a minimum greater than its maximum"),
        ("--bbox=-10,-89,10,89", "latitude"),
    ] {
        let log = MartinCp::bulk()
            .with_postgres()
            .arg("--output-file")
            .arg(dir.path().join("out.mbtiles"))
            .arg(bbox)
            .run_expecting_failure()
            .await;
        assert!(log.contains(expected), "{log}");
    }
}

#[tokio::test]
async fn aborts_on_a_misconfigured_table() {
    let dir = temp_dir();
    let config = dir.path().join("config.yaml");
    fs::write(
        &config,
        indoc! {"
            postgres:
              connection_string: ${DATABASE_URL}
              auto_publish: false
              tables:
                table_source:
                  schema: public
                  table: table_source
                  srid: 4326
                  geometry_column: geom
                  geometry_type: GEOMETRY
                typo:
                  schema: ghost_schema
                  table: table_source
                  srid: 4326
                  geometry_column: geom
                  geometry_type: GEOMETRY
        "},
    )
    .expect("failed to write the config");
    let output = dir.path().join("out.mbtiles");
    let log = MartinCp::bulk()
        .env("RUST_LOG", "error")
        .with_postgres()
        .arg("--config")
        .arg(&config)
        .arg("--output-file")
        .arg(&output)
        .run_expecting_failure()
        .await;
    insta::assert_snapshot!(log, @r#"
    ERROR Tile source resolution warning warning=Source typo: Unable to configure source typo because schema 'ghost_schema' does not exist. Available schemas: "Quotes' \ \' \" and Space.Dot., MixedCase, autodetect, public, schema_a, schema_b, topology
    ERROR error=TileResolutionWarningsIssued
    ERROR warnings issued during tile source resolution
    "#);
    assert!(!output.exists());
}

#[tokio::test]
async fn warns_about_a_misconfigured_table_with_on_invalid_warn() {
    let dir = temp_dir();
    let config = dir.path().join("config.yaml");
    fs::write(
        &config,
        indoc! {"
            on_invalid: warn
            postgres:
              connection_string: ${DATABASE_URL}
              auto_publish: false
              tables:
                table_source:
                  schema: public
                  table: table_source
                  filter: \"gid <> 14\"
                  srid: 4326
                  geometry_column: geom
                  geometry_type: GEOMETRY
                typo:
                  schema: ghost_schema
                  table: table_source
                  srid: 4326
                  geometry_column: geom
                  geometry_type: GEOMETRY
        "},
    )
    .expect("failed to write the config");
    let output = dir.path().join("out.mbtiles");
    let log = MartinCp::bulk()
        .env("RUST_LOG", "warn")
        .with_postgres()
        .arg("--config")
        .arg(&config)
        .arg("--output-file")
        .arg(&output)
        .arg("--max-zoom")
        .arg("1")
        .run()
        .await;
    insta::assert_snapshot!(log, @r#"
    WARN Table has no spatial index on the geometry column schema=public table=table_source geometry_column=geom
    WARN Tile source resolution warning warning=Source typo: Unable to configure source typo because schema 'ghost_schema' does not exist. Available schemas: "Quotes' \ \' \" and Space.Dot., MixedCase, autodetect, public, schema_a, schema_b, topology
    "#);
    assert!(output.exists());
}

#[tokio::test]
async fn aborts_on_a_geometry_collection_naming_the_source_and_a_query_counting_such_rows() {
    let dir = temp_dir();
    let config = dir.path().join("config.yaml");
    fs::write(
        &config,
        indoc! {"
            postgres:
              connection_string: ${DATABASE_URL}
              auto_publish: false
              tables:
                table_source:
                  schema: public
                  table: table_source
                  filter: \"gid >= 14\"
                  srid: 4326
                  geometry_column: geom
                  geometry_type: GEOMETRY
                  id_column: gid
        "},
    )
    .expect("failed to write the config");
    let output = dir.path().join("out.mbtiles");
    let log = MartinCp::bulk()
        .env("RUST_LOG", "error")
        .with_postgres()
        .arg("--config")
        .arg(&config)
        .arg("--output-file")
        .arg(&output)
        .run_expecting_failure()
        .await;
    insta::assert_snapshot!(log, @r#"
    ERROR Source `table_source`: feature 14: a GEOMETRYCOLLECTION cannot go into a tile, which holds only points, lines and polygons (single or multi)
    Count the rows of each geometry type it cannot render with:
      SELECT ST_GeometryType(g), count(*) FROM (SELECT ST_CurveToLine("geom"::geometry) AS g FROM "public"."table_source" WHERE "geom" IS NOT NULL AND (gid >= 14)) AS t WHERE ST_GeometryType(g) NOT IN ('ST_Point', 'ST_MultiPoint', 'ST_LineString', 'ST_MultiLineString', 'ST_Polygon', 'ST_MultiPolygon') AND NOT (ST_GeometryType(g) = 'ST_GeometryCollection' AND ST_IsEmpty(g)) GROUP BY 1
    "#);
    assert!(!output.exists());
}

#[tokio::test]
async fn logs_the_postgres_error_when_a_scan_fails() {
    let dir = temp_dir();
    let config = dir.path().join("config.yaml");
    fs::write(
        &config,
        indoc! {"
            postgres:
              connection_string: ${DATABASE_URL}
              auto_publish: false
              tables:
                table_source:
                  schema: public
                  table: table_source
                  srid: 4326
                  geometry_column: geom
                  geometry_type: GEOMETRY
                  filter: \"gid = 'abc'\"
        "},
    )
    .expect("failed to write the config");
    let log = MartinCp::bulk()
        .env("RUST_LOG", "error")
        .with_postgres()
        .arg("--config")
        .arg(&config)
        .arg("--output-file")
        .arg(dir.path().join("out.mbtiles"))
        .run_expecting_failure()
        .await;
    insta::assert_snapshot!(log, @r#"ERROR Postgres error while starting a scan: db error: ERROR: invalid input syntax for type integer: "abc""#);
}

#[cfg(unix)]
#[tokio::test]
async fn sigterm_removes_the_partial_output() {
    let dir = temp_dir();
    let output = dir.path().join("out.mbtiles");
    let log = MartinCp::bulk()
        .env("RUST_LOG", "error")
        .with_postgres()
        .arg("--output-file")
        .arg(&output)
        .run_until_terminated_once_created(&output)
        .await;
    let dir_path = regex::escape(&dir.path().display().to_string());
    insta::with_settings!({ filters => vec![(dir_path.as_str(), "[DIR]")] }, {
        insta::assert_snapshot!(log, @"ERROR Interrupted; removed the partial output [DIR]/out.mbtiles");
    });
    let left: Vec<_> = fs::read_dir(dir.path())
        .expect("failed to list the output directory")
        .map(|entry| entry.expect("failed to read a directory entry").file_name())
        .collect();
    insta::assert_debug_snapshot!(left, @"[]");
}
