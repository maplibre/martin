//! Tile postprocessing: `convert_to_mlt` encoder settings, and the `passthrough` source proxying another martin.

#![cfg(feature = "test-pg")]

use std::fs;

use martin_integration_tests::Martin;
use tempfile::TempDir;

const MLT_ACCEPT: &str = "application/vnd.maplibre-tile";

const TESSELLATED: &str = "
      convert_to_mlt:
        tessellate: true";

fn tile_sources(table_source_encoder: &str) -> String {
    format!(
        r"
convert_to_mlt: auto
postgres:
  connection_string: ${{DATABASE_URL}}
  default_srid: 4326
  pool_size: 1
  auto_bounds: skip
  tables:
    table_source:
      schema: public
      table: table_source
      srid: 4326
      geometry_column: geom
      geometry_type: GEOMETRY
      properties:
        gid: int4{table_source_encoder}
    points2:
      schema: public
      table: points2
      srid: 4326
      geometry_column: geom
      geometry_type: POINT
      properties:
        gid: int4
      convert_to_mlt:
        try_spatial_hilbert_sort: true
"
    )
}

const PASSTHROUGH_SOURCE: &str = r"
convert_to_mlt: auto
passthrough:
  sources:
    passthrough_table:
      url: http://UPSTREAM/table_source/{z}/{x}/{y}
      format: mvt
";

async fn martin_serving_tiles() -> (TempDir, Martin) {
    start(&tile_sources(TESSELLATED), true, None).await
}

async fn martin_proxying(upstream: &Martin) -> (TempDir, Martin) {
    let config = PASSTHROUGH_SOURCE.replace("UPSTREAM", upstream.addr());
    start(&config, false, None).await
}

async fn start(config: &str, postgres: bool, save_config: Option<&str>) -> (TempDir, Martin) {
    let dir = tempfile::tempdir().expect("failed to create a temp dir");
    let path = dir.path().join("config.yaml");
    fs::write(&path, config).expect("failed to write the config file");

    let mut builder = Martin::builder().arg("--config").arg(&path);
    if postgres {
        builder = builder.with_postgres();
    }
    if let Some(name) = save_config {
        builder = builder.arg("--save-config").arg(dir.path().join(name));
    }
    let martin = builder.start().await.expect("failed to start martin");
    (dir, martin)
}

fn assert_table_source_warning(martin: &mut Martin) {
    martin.assert_log_contains("Table public.table_source has no spatial index on column geom");
}

#[tokio::test]
async fn the_catalog_lists_every_configured_source() {
    let (_dir, mut martin) = martin_serving_tiles().await;

    insta::assert_json_snapshot!(martin.get("/catalog").await.json()["tiles"], @r#"
    {
      "points2": {
        "content_type": "application/x-protobuf",
        "description": "public.points2.geom"
      },
      "table_source": {
        "content_type": "application/x-protobuf"
      }
    }
    "#);

    martin.stop().await;
    assert_table_source_warning(&mut martin);
    martin.assert_log_clean();
}

#[tokio::test]
async fn the_saved_config_records_the_resolved_encoder_settings() {
    let (dir, mut martin) = start(&tile_sources(TESSELLATED), true, Some("save_config.yaml")).await;

    let saved = fs::read_to_string(dir.path().join("save_config.yaml"))
        .expect("martin did not write --save-config");
    let saved = saved
        .lines()
        .filter(|line| !line.contains("connection_string:"))
        .collect::<Vec<_>>()
        .join("\n");
    insta::assert_snapshot!(saved, @"
    listen_addresses: 127.0.0.1:0
    postgres:
      default_srid: 4326
      auto_bounds: skip
      pool_size: 1
      tables:
        points2:
          schema: public
          table: points2
          srid: 4326
          geometry_column: geom
          geometry_type: POINT
          properties:
            gid: int4
          convert_to_mlt:
            try_spatial_hilbert_sort: true
        table_source:
          schema: public
          table: table_source
          srid: 4326
          geometry_column: geom
          geometry_type: GEOMETRY
          properties:
            gid: int4
          convert_to_mlt:
            tessellate: true
      functions: {}
    convert_to_mlt: auto
    ");

    martin.stop().await;
    assert_table_source_warning(&mut martin);
    martin.assert_log_clean();
}

#[tokio::test]
async fn an_mlt_accept_header_converts_the_tile_and_suffixes_its_etag() {
    let (_dir, mut martin) = martin_serving_tiles().await;

    let mvt = martin.get("/table_source/0/0/0").await;
    let mlt = martin
        .get_with_headers("/table_source/0/0/0", &[("accept", MLT_ACCEPT)])
        .await;

    assert_eq!(mlt.status(), 200);
    insta::assert_snapshot!(mlt.headers_snapshot(), @r#"
    content-length: 916
    content-type: application/vnd.maplibre-tile
    etag: "lDkxk7p2r2HsWFzhXDsD5A+mlt"
    vary: Origin, Access-Control-Request-Method, Access-Control-Request-Headers
    "#);
    let mvt_etag = mvt.header("etag").expect("the mvt response has no etag");
    assert_eq!(
        mlt.header("etag"),
        Some(format!("{}+mlt\"", mvt_etag.trim_end_matches('"')).as_str())
    );

    let layers = mlt.mlt();
    assert_eq!(layers.len(), 1);
    assert_eq!(layers[0].name(), "table_source");
    assert_eq!(layers[0].extent().get(), 4096);
    assert_eq!(layers[0].property_names(), ["gid"]);
    assert_eq!(
        layers[0].features().len(),
        mvt.mvt().layers[0].features.len()
    );

    martin.stop().await;
    assert_table_source_warning(&mut martin);
    martin.assert_log_clean();
}

#[tokio::test]
async fn a_per_source_encoder_setting_overrides_the_global_default() {
    let (_dir, mut tessellating) = martin_serving_tiles().await;
    let (_plain_dir, mut plain) = start(&tile_sources(""), true, None).await;

    let with_triangles = tessellating
        .get_with_headers("/table_source/0/0/0", &[("accept", MLT_ACCEPT)])
        .await;
    let without_triangles = plain
        .get_with_headers("/table_source/0/0/0", &[("accept", MLT_ACCEPT)])
        .await;

    assert!(
        with_triangles.body().len() > without_triangles.body().len(),
        "the tessellated tile ({}B) must carry more than the same geometry without triangles ({}B)",
        with_triangles.body().len(),
        without_triangles.body().len()
    );
    assert_eq!(
        with_triangles.mlt()[0].features().len(),
        without_triangles.mlt()[0].features().len()
    );

    for martin in [&mut tessellating, &mut plain] {
        martin.stop().await;
        assert_table_source_warning(martin);
        martin.assert_log_clean();
    }
}

#[tokio::test]
async fn a_passthrough_source_serves_the_upstream_tile_verbatim() {
    let (_dir, mut upstream) = martin_serving_tiles().await;
    let (_proxy_dir, mut proxy) = martin_proxying(&upstream).await;

    insta::assert_json_snapshot!(proxy.get("/catalog").await.json()["tiles"], @r#"
    {
      "passthrough_table": {
        "content_type": "application/x-protobuf"
      }
    }
    "#);

    let direct = upstream.get("/table_source/0/0/0").await;
    let proxied = proxy.get("/passthrough_table/0/0/0").await;
    assert_eq!(proxied.status(), 200);
    insta::assert_snapshot!(proxied.headers_snapshot(), @r#"
    content-encoding: gzip
    content-length: 643
    content-type: application/x-protobuf
    etag: "lDkxk7p2r2HsWFzhXDsD5A"
    vary: Origin, Access-Control-Request-Method, Access-Control-Request-Headers
    "#);
    assert_eq!(proxied.header("etag"), direct.header("etag"));
    assert_eq!(proxied.body(), direct.body());

    proxy.stop().await;
    proxy.assert_log_clean();
    upstream.stop().await;
    assert_table_source_warning(&mut upstream);
    upstream.assert_log_clean();
}

#[tokio::test]
async fn a_passthrough_source_converts_the_proxied_tile_to_mlt() {
    let (_dir, mut upstream) = martin_serving_tiles().await;
    let (_proxy_dir, mut proxy) = martin_proxying(&upstream).await;

    let proxied = proxy
        .get_with_headers("/passthrough_table/0/0/0", &[("accept", MLT_ACCEPT)])
        .await;
    assert_eq!(proxied.status(), 200);
    insta::assert_snapshot!(proxied.headers_snapshot(), @r#"
    content-length: 645
    content-type: application/vnd.maplibre-tile
    etag: "lDkxk7p2r2HsWFzhXDsD5A+mlt"
    vary: Origin, Access-Control-Request-Method, Access-Control-Request-Headers
    "#);

    let direct = upstream.get("/table_source/0/0/0").await;
    let layers = proxied.mlt();
    assert_eq!(layers.len(), 1);
    assert_eq!(layers[0].name(), "table_source");
    assert_eq!(
        layers[0].features().len(),
        direct.mvt().layers[0].features.len()
    );

    proxy.stop().await;
    proxy.assert_log_clean();
    upstream.stop().await;
    assert_table_source_warning(&mut upstream);
    upstream.assert_log_clean();
}
