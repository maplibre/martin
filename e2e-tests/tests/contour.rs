use std::fs;
use std::path::PathBuf;

use geojson::GeometryValue;
use image::ImageFormat;
use martin_e2e_tests::{Martin, StaticFiles, fixture};
use martin_tile_utils::tile_index;
use serde_json::json;

fn neighbour_tile(x: u32, y: u32) -> PathBuf {
    fixture(&format!("terrain/terrarium/10_{x}_{y}.png"))
}

fn neighbourhood() -> Vec<(u32, u32, u32)> {
    (-1i32..=1)
        .flat_map(|dy| {
            (-1i32..=1)
                .map(move |dx| (10, 163u32.wrapping_add_signed(dx), 396u32.wrapping_add_signed(dy)))
        })
        .collect()
}

async fn upstream() -> StaticFiles {
    let paths = neighbourhood()
        .into_iter()
        .map(|(z, x, y)| (format!("{z}/{x}/{y}"), neighbour_tile(x, y)))
        .collect::<Vec<_>>();
    let files = paths
        .iter()
        .map(|(path, tile)| (path.as_str(), tile.clone()))
        .collect::<Vec<_>>();
    StaticFiles::serving(&files).await
}

async fn start(files: &StaticFiles, contour: &str) -> Martin {
    Martin::builder()
        .config(&format!(
            "passthrough:
  sources:
    elevation:
      url: {}/{{z}}/{{x}}/{{y}}
      format: png
      maxzoom: 12
      {contour}
",
            files.base_url()
        ))
        .start()
        .await
        .expect("failed to start martin")
}

fn tile_path() -> String {
    "/elevation/10/163/396".to_owned()
}

async fn sorted_log(files: &StaticFiles) -> String {
    let mut lines = files
        .request_log()
        .await
        .lines()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    lines.sort();
    lines.join("\n")
}

#[tokio::test]
async fn an_elevation_source_is_served_as_traced_contours() {
    let files = upstream().await;
    let mut martin = start(&files, "convert_to_contour: auto").await;

    let response = martin.get(&tile_path()).await;
    assert_eq!(response.status(), 200);
    insta::with_settings!({filters => vec![
        (r"(?m)^content-length: \d+$", "content-length: [LENGTH]"),
        (r"(?m)^etag: .*$", "etag: [ETAG]"),
    ]}, {
        insta::assert_snapshot!(response.headers_snapshot(), @"
        content-encoding: gzip
        content-length: [LENGTH]
        content-type: application/x-protobuf
        etag: [ETAG]
        vary: Origin, Access-Control-Request-Method, Access-Control-Request-Headers
        ");
    });
    let layers = response.mvt().layers;
    assert_eq!(layers.len(), 1);
    assert_eq!(layers[0].name, "contour");
    assert_eq!(layers[0].extent.get(), 4096);
    assert!(!layers[0].features.is_empty(), "real terrain should trace at least one line");
    let tags = layers[0].features[0]
        .properties
        .iter()
        .map(|(key, _)| key.as_str())
        .collect::<Vec<_>>();
    assert_eq!(tags, ["ele", "major"]);
    martin.stop().await;
}

#[tokio::test]
async fn a_traced_tile_reprojects_to_contour_lines_over_the_fixture_tile() {
    let files = upstream().await;
    let mut martin = start(&files, "convert_to_contour: auto").await;

    let response = martin.get(&tile_path()).await;
    assert_eq!(response.status(), 200);

    let collection = response.geojson(10, 163, 396);
    assert!(!collection.features.is_empty());
    for feature in &collection.features {
        let geometry = feature.geometry.as_ref().expect("a feature has geometry");
        let GeometryValue::LineString { coordinates } = &geometry.value else {
            panic!("expected a linestring, got {}", geometry.value.type_name());
        };
        assert!(coordinates.len() >= 2, "{coordinates:?}");
        for position in coordinates {
            let (column, row) = tile_index(position[0], position[1], 10);
            assert!(
                column.abs_diff(163) <= 1 && row.abs_diff(396) <= 1,
                "{position:?} reprojects to 10/{column}/{row}, off the fixture neighbourhood"
            );
        }

        let properties = feature.properties.as_ref().expect("a feature has tags");
        let elevation = properties["ele"].as_i64().expect("an integer elevation");
        assert_eq!(elevation % 100, 0, "{elevation} m is off the z10 interval");
        assert_eq!(properties["major"], json!(elevation % 500 == 0));
    }

    insta::assert_snapshot!(response.geojson_dump(10, 163, 396));
    martin.stop().await;
}

#[tokio::test]
async fn tracing_one_tile_reads_exactly_nine_upstream_tiles() {
    let files = upstream().await;
    let mut martin = start(&files, "convert_to_contour: auto").await;

    assert_eq!(martin.get(&tile_path()).await.status(), 200);

    let log = sorted_log(&files).await;
    let requests = log.lines().filter(|line| !line.is_empty()).count();
    assert_eq!(
        requests, 9,
        "one trace must read nine tiles; {requests} means the gather re-entered \
         the contour pass.\n{log}"
    );
    insta::assert_snapshot!(log, @"
    GET /10/162/395 no range
    GET /10/162/396 no range
    GET /10/162/397 no range
    GET /10/163/395 no range
    GET /10/163/396 no range
    GET /10/163/397 no range
    GET /10/164/395 no range
    GET /10/164/396 no range
    GET /10/164/397 no range
    ");
    martin.stop().await;
}

#[tokio::test]
async fn a_disabled_source_serves_the_elevation_tile_unchanged() {
    let files = upstream().await;
    let mut martin = start(&files, "convert_to_contour: disabled").await;

    let response = martin.get(&tile_path()).await;
    assert_eq!(response.status(), 200);
    assert_eq!(response.header("content-type"), Some("image/png"));
    assert_eq!(response.image_format(), ImageFormat::Png);
    assert_eq!(response.image_size(), (256, 256));
    let upstream_bytes = fs::read(neighbour_tile(163, 396)).expect("failed to read the fixture");
    assert_eq!(
        response.body(),
        upstream_bytes,
        "a disabled pass must pass the raw elevation tile through"
    );
    martin.stop().await;
}

#[tokio::test]
async fn an_out_of_range_setting_is_rejected_at_startup() {
    let files = upstream().await;
    let started = Martin::builder()
        .config(&format!(
            "passthrough:
  sources:
    elevation:
      url: {}/{{z}}/{{x}}/{{y}}
      format: png
      maxzoom: 12
      convert_to_contour:
        resolution: 0
",
            files.base_url()
        ))
        .start()
        .await;
    assert!(started.is_err(), "a resolution outside 1-64 must not start the server");
}

#[tokio::test]
async fn query_overrides_are_ignored_unless_opted_in() {
    let files = upstream().await;
    let mut martin = start(&files, "convert_to_contour: auto").await;

    let plain = martin.get(&tile_path()).await;
    let overridden = martin
        .get(&format!("{}?simplification_tolerance=50", tile_path()))
        .await;

    assert_eq!(plain.status(), 200);
    assert_eq!(overridden.status(), 200);
    assert_eq!(
        plain.body(),
        overridden.body(),
        "without allow_request_overrides the query must not change the trace"
    );
    martin.stop().await;
}

#[tokio::test]
async fn a_contoured_source_advertises_vector_tiles() {
    let files = upstream().await;
    let mut martin = start(&files, "convert_to_contour: auto").await;

    let catalog = martin.get("/catalog").await.json();
    insta::assert_json_snapshot!(catalog["tiles"]["elevation"], @r#"
    {
      "content_type": "application/x-protobuf"
    }
    "#);

    let response = martin.get("/elevation").await;
    let tilejson = serde_json::from_str::<serde_json::Value>(&martin.redact(&response.text()))
        .expect("response body is not valid json");
    insta::assert_json_snapshot!(tilejson, @r#"
    {
      "maxzoom": 12,
      "tilejson": "3.0.0",
      "tiles": [
        "http://[ADDR]/elevation/{z}/{x}/{y}"
      ],
      "vector_layers": [
        {
          "description": "Contour lines, elevation in meters",
          "fields": {
            "ele": "Number",
            "major": "Boolean"
          },
          "id": "contour",
          "maxzoom": 12
        }
      ]
    }
    "#);
    martin.stop().await;
}

#[tokio::test]
async fn a_contoured_source_accepts_a_vector_tile_accept_header() {
    let files = upstream().await;
    let mut martin = start(&files, "convert_to_contour: auto").await;

    let response = martin
        .get_with_headers(&tile_path(), &[("Accept", "application/vnd.mapbox-vector-tile")])
        .await;
    assert_eq!(
        response.status(),
        200,
        "negotiation must run against the traced format, not the elevation source's"
    );
    assert_eq!(response.header("content-type"), Some("application/x-protobuf"));
    assert_eq!(response.mvt().layers[0].name, "contour");
    martin.stop().await;
}

#[tokio::test]
async fn a_contoured_source_transcodes_to_mlt_on_request() {
    let files = upstream().await;
    let mut martin = start(&files, "convert_to_contour: auto\n      convert_to_mlt: auto").await;

    let response = martin
        .get_with_headers(&tile_path(), &[("Accept", "application/vnd.maplibre-tile")])
        .await;
    assert_eq!(response.status(), 200);
    assert_eq!(
        response.header("content-type"),
        Some("application/vnd.maplibre-tile"),
        "the traced tile must run through the MLT transcoder like any other MVT"
    );
    let layers = response.mlt();
    assert_eq!(layers.len(), 1);
    assert_eq!(layers[0].name(), "contour");
    martin.stop().await;
}

#[tokio::test]
async fn a_disabled_source_still_advertises_the_raster() {
    let files = upstream().await;
    let mut martin = start(&files, "convert_to_contour: disabled").await;

    let catalog = martin.get("/catalog").await.json();
    assert_eq!(catalog["tiles"]["elevation"]["content_type"], "image/png");

    let tilejson = martin.get("/elevation").await.json();
    assert!(tilejson["vector_layers"].is_null());
    martin.stop().await;
}
