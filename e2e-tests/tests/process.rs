//! Tile postprocessing: `convert_to_mlt` encoder settings, and the `passthrough` source proxying another martin.

#![cfg(feature = "test-pg")]

use std::fs;

use martin_e2e_tests::Martin;

const MLT_ACCEPT: &str = "application/vnd.maplibre-tile";

fn assert_table_source_warning(martin: &mut Martin) {
    martin.assert_log_contains("Table has no spatial index on the geometry column schema=public table=table_source geometry_column=geom");
}

#[tokio::test]
async fn the_catalog_lists_every_configured_source() {
    let mut martin = Martin::builder()
        .config(
            r"
convert_to_mlt: auto
postgres:
  connection_string: ${DATABASE_URL}
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
        gid: int4
      convert_to_mlt:
        tessellate: true
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
",
        )
        .with_postgres()
        .start()
        .await
        .expect("failed to start martin");

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
}

#[tokio::test]
async fn the_saved_config_records_the_resolved_encoder_settings() {
    let dir = tempfile::tempdir().expect("failed to create a temp dir");
    let mut martin = Martin::builder()
        .config(
            r"
convert_to_mlt: auto
postgres:
  connection_string: ${DATABASE_URL}
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
        gid: int4
      convert_to_mlt:
        tessellate: true
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
",
        )
        .with_postgres()
        .arg("--save-config")
        .arg(dir.path().join("save_config.yaml"))
        .start()
        .await
        .expect("failed to start martin");

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
}

#[tokio::test]
async fn an_mlt_accept_header_converts_the_tile_and_suffixes_its_etag() {
    let mut martin = Martin::builder()
        .config(
            r"
convert_to_mlt: auto
postgres:
  connection_string: ${DATABASE_URL}
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
        gid: int4
      convert_to_mlt:
        tessellate: true
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
",
        )
        .with_postgres()
        .start()
        .await
        .expect("failed to start martin");

    let mvt = martin.get("/table_source/0/0/0").await;
    let mlt = martin
        .get_with_headers("/table_source/0/0/0", &[("accept", MLT_ACCEPT)])
        .await;

    assert_eq!(mlt.status(), 200);
    insta::assert_snapshot!(mlt.headers_snapshot(), @r#"
    content-length: 927
    content-type: application/vnd.maplibre-tile
    etag: "lDkxk7p2r2HsWFzhXDsD5A+mlt"
    vary: accept-encoding, Origin, Access-Control-Request-Method, Access-Control-Request-Headers
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
}

#[tokio::test]
async fn a_per_source_encoder_setting_overrides_the_global_default() {
    let mut tessellating = Martin::builder()
        .config(
            r"
convert_to_mlt: auto
postgres:
  connection_string: ${DATABASE_URL}
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
        gid: int4
      convert_to_mlt:
        tessellate: true
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
",
        )
        .with_postgres()
        .start()
        .await
        .expect("failed to start martin");
    let mut plain = Martin::builder()
        .config(
            r"
convert_to_mlt: auto
postgres:
  connection_string: ${DATABASE_URL}
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
        gid: int4
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
",
        )
        .with_postgres()
        .start()
        .await
        .expect("failed to start martin");

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
    }
}

#[tokio::test]
async fn a_passthrough_source_serves_the_upstream_tile_verbatim() {
    let mut upstream = Martin::builder()
        .config(
            r"
convert_to_mlt: auto
postgres:
  connection_string: ${DATABASE_URL}
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
        gid: int4
      convert_to_mlt:
        tessellate: true
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
",
        )
        .with_postgres()
        .start()
        .await
        .expect("failed to start martin");
    let mut proxy = Martin::builder()
        .config(&format!(
            r"
convert_to_mlt: auto
passthrough:
  sources:
    passthrough_table:
      url: http://{}/table_source/{{z}}/{{x}}/{{y}}
      format: mvt
",
            upstream.addr()
        ))
        .start()
        .await
        .expect("failed to start martin");

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
    content-length: 678
    content-type: application/x-protobuf
    etag: "lDkxk7p2r2HsWFzhXDsD5A"
    vary: accept-encoding, Origin, Access-Control-Request-Method, Access-Control-Request-Headers
    "#);
    assert_eq!(proxied.header("etag"), direct.header("etag"));
    assert_eq!(proxied.body(), direct.body());

    proxy.stop().await;
    upstream.stop().await;
    assert_table_source_warning(&mut upstream);
}

#[tokio::test]
async fn a_passthrough_source_converts_the_proxied_tile_to_mlt() {
    let mut upstream = Martin::builder()
        .config(
            r"
convert_to_mlt: auto
postgres:
  connection_string: ${DATABASE_URL}
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
        gid: int4
      convert_to_mlt:
        tessellate: true
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
",
        )
        .with_postgres()
        .start()
        .await
        .expect("failed to start martin");
    let mut proxy = Martin::builder()
        .config(&format!(
            r"
convert_to_mlt: auto
passthrough:
  sources:
    passthrough_table:
      url: http://{}/table_source/{{z}}/{{x}}/{{y}}
      format: mvt
",
            upstream.addr()
        ))
        .start()
        .await
        .expect("failed to start martin");

    let proxied = proxy
        .get_with_headers("/passthrough_table/0/0/0", &[("accept", MLT_ACCEPT)])
        .await;
    assert_eq!(proxied.status(), 200);
    insta::assert_snapshot!(proxied.headers_snapshot(), @r#"
    content-length: 696
    content-type: application/vnd.maplibre-tile
    etag: "lDkxk7p2r2HsWFzhXDsD5A+mlt"
    vary: accept-encoding, Origin, Access-Control-Request-Method, Access-Control-Request-Headers
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
    upstream.stop().await;
    assert_table_source_warning(&mut upstream);
}
