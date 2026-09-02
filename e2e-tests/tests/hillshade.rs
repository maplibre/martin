use std::fs;
use std::path::PathBuf;

use martin_e2e_tests::{
    Martin, StartError, StaticFiles, assert_image_matches, assert_images_differ, fixture,
};

const CENTRE: (u32, u32) = (163, 396);
const ZOOM: u32 = 10;

fn normal_tile() -> PathBuf {
    neighbour_tile(CENTRE.0, CENTRE.1)
}

fn neighbour_tile(x: u32, y: u32) -> PathBuf {
    fixture(&format!("terrain/normal/{ZOOM}_{x}_{y}.png"))
}

fn reference(name: &str) -> PathBuf {
    fixture(&format!("hillshade_references/{name}"))
}

fn centre() -> (u32, u32, u32) {
    (ZOOM, CENTRE.0, CENTRE.1)
}

fn neighbourhood() -> Vec<(u32, u32, u32)> {
    (-1i32..=1)
        .flat_map(|dy| {
            (-1i32..=1).map(move |dx| {
                (
                    ZOOM,
                    CENTRE.0.wrapping_add_signed(dx),
                    CENTRE.1.wrapping_add_signed(dy),
                )
            })
        })
        .collect()
}

async fn serving(coords: impl IntoIterator<Item = (u32, u32, u32)>) -> StaticFiles {
    let paths = coords
        .into_iter()
        .map(|(z, x, y)| (format!("{z}/{x}/{y}"), neighbour_tile(x, y)))
        .collect::<Vec<_>>();
    let files = paths
        .iter()
        .map(|(path, tile)| (path.as_str(), tile.clone()))
        .collect::<Vec<_>>();
    StaticFiles::serving(&files).await
}

async fn upstream() -> StaticFiles {
    serving(neighbourhood()).await
}

async fn start(files: &StaticFiles, hillshade: &str) -> Martin {
    start_with(files, hillshade, "").await
}

async fn start_with(files: &StaticFiles, hillshade: &str, extra: &str) -> Martin {
    Martin::builder()
        .config(&format!(
            "passthrough:
  sources:
    terrain:
      url: {}/{{z}}/{{x}}/{{y}}
      format: png
      maxzoom: 12
{extra}      convert_to_hillshade:{hillshade}
",
            files.base_url()
        ))
        .start()
        .await
        .expect("failed to start martin")
}

fn tile_path() -> String {
    format!("/terrain/{ZOOM}/{}/{}", CENTRE.0, CENTRE.1)
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
async fn a_normal_source_is_served_as_a_baked_hillshade() {
    let files = upstream().await;
    let mut martin = start(&files, " auto").await;

    let response = martin.get(&tile_path()).await;
    assert_eq!(response.status(), 200);
    assert_eq!(response.image_size(), (512, 512));
    insta::with_settings!({filters => vec![
        (r"(?m)^content-length: \d+$", "content-length: [LENGTH]"),
        (r"(?m)^etag: .*$", "etag: [ETAG]"),
    ]}, {
        insta::assert_snapshot!(response.headers_snapshot(), @"
        content-length: [LENGTH]
        content-type: image/png
        etag: [ETAG]
        vary: Origin, Access-Control-Request-Method, Access-Control-Request-Headers
        ");
    });
    assert_image_matches(reference("baked.png"), response.body());
    martin.stop().await;
}

#[tokio::test]
async fn baking_one_tile_reads_exactly_nine_upstream_tiles() {
    let files = upstream().await;
    let mut martin = start(&files, " auto").await;

    assert_eq!(martin.get(&tile_path()).await.status(), 200);

    let log = sorted_log(&files).await;
    let requests = log.lines().filter(|line| !line.is_empty()).count();
    assert_eq!(
        requests, 9,
        "one bake must read nine tiles; {requests} means the gather re-entered \
         the hillshade pass.\n{log}"
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
async fn neighbour_reads_carry_no_query_string() {
    let files = upstream().await;
    let mut martin = start(&files, "\n        allow_request_overrides: true").await;

    let path = format!("{}?azimuth=180&ambient=0.4", tile_path());
    assert_eq!(martin.get(&path).await.status(), 200);

    insta::assert_snapshot!(sorted_log(&files).await, @"
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
async fn a_second_request_reuses_the_cached_normal_tiles() {
    let files = upstream().await;
    let mut martin = start(&files, " auto").await;

    assert_eq!(martin.get(&tile_path()).await.status(), 200);
    let after_first = files.request_log().await.lines().count();

    assert_eq!(martin.get(&tile_path()).await.status(), 200);
    let after_second = files.request_log().await.lines().count();

    assert_eq!(
        after_first, after_second,
        "the second bake must read its inputs from the cache, not upstream"
    );
    martin.stop().await;
}

#[tokio::test]
async fn a_source_that_forbids_caching_re_reads_its_normal_tiles() {
    let files = upstream().await;
    let mut martin = start_with(&files, " auto", "      cache: disable\n").await;

    assert_eq!(martin.get(&tile_path()).await.status(), 200);
    assert_eq!(martin.get(&tile_path()).await.status(), 200);

    let requests = files.request_log().await.lines().count();
    assert_eq!(
        requests, 18,
        "a source that forbids caching must have its normal tiles read again"
    );
    martin.stop().await;
}

#[tokio::test]
async fn an_apron_widens_the_served_tile() {
    let files = upstream().await;
    let mut martin = start(&files, "\n        padding: 8").await;

    let response = martin.get(&tile_path()).await;
    assert_eq!(response.image_size(), (544, 544));
    assert_image_matches(reference("apron.png"), response.body());
    martin.stop().await;
}

#[tokio::test]
async fn webp_is_served_when_configured() {
    let files = upstream().await;
    let mut martin = start(&files, "\n        format: webp").await;

    let response = martin.get(&tile_path()).await;
    assert_eq!(response.status(), 200);
    assert_eq!(response.header("content-type"), Some("image/webp"));
    assert_eq!(response.image_size(), (512, 512));
    assert_image_matches(reference("baked.webp"), response.body());
    martin.stop().await;
}

#[tokio::test]
async fn jxl_is_served_when_configured() {
    let files = upstream().await;
    let mut martin = start(&files, "\n        format: jxl").await;

    let response = martin.get(&tile_path()).await;
    assert_eq!(response.status(), 200);
    assert_eq!(response.header("content-type"), Some("image/jxl"));
    assert_eq!(response.image_size(), (512, 512));
    assert_image_matches(reference("baked.jxl"), response.body());
    martin.stop().await;
}

#[tokio::test]
async fn a_disabled_hillshade_serves_the_source_unchanged() {
    let files = upstream().await;
    let mut martin = start(&files, " disabled").await;

    let response = martin.get(&tile_path()).await;
    let upstream_bytes = fs::read(normal_tile()).expect("failed to read the fixture");
    assert_eq!(response.body(), upstream_bytes, "the upstream's own bytes");
    insta::assert_snapshot!(sorted_log(&files).await, @"GET /10/163/396 no range");
    martin.stop().await;
}

#[tokio::test]
async fn a_degraded_neighbourhood_still_serves() {
    let files = serving([centre()]).await;
    let mut martin = start(&files, " auto").await;

    let response = martin.get(&tile_path()).await;
    assert_eq!(response.status(), 200);
    assert_eq!(response.image_size(), (512, 512));
    assert_image_matches(reference("degraded_neighbourhood.png"), response.body());
    martin.stop().await;
}

#[tokio::test]
async fn an_absent_centre_tile_serves_no_content() {
    let files = serving(neighbourhood().into_iter().filter(|c| *c != centre())).await;
    let mut martin = start(&files, " auto").await;

    let response = martin.get(&tile_path()).await;
    assert_eq!(response.status(), 204);
    assert!(
        response.body().is_empty(),
        "an absent centre must not be answered with a baked tile"
    );
    martin.stop().await;
}

#[tokio::test]
async fn a_tile_at_the_pole_still_serves() {
    let tile = normal_tile();
    let coords = (0..4).flat_map(|x| (0..4).map(move |y| (2, x, y)));
    let paths = coords
        .map(|(z, x, y)| format!("{z}/{x}/{y}"))
        .collect::<Vec<_>>();
    let entries = paths
        .iter()
        .map(|path| (path.as_str(), tile.clone()))
        .collect::<Vec<_>>();
    let files = StaticFiles::serving(&entries).await;
    let mut martin = start(&files, " auto").await;

    let response = martin.get("/terrain/2/1/0").await;
    assert_eq!(response.status(), 200);
    assert_eq!(response.image_size(), (512, 512));
    insta::assert_snapshot!(sorted_log(&files).await, @"
    GET /2/0/0 no range
    GET /2/0/1 no range
    GET /2/1/0 no range
    GET /2/1/1 no range
    GET /2/2/0 no range
    GET /2/2/1 no range
    ");
    martin.stop().await;
}

#[tokio::test]
async fn request_overrides_are_refused_unless_configured() {
    let files = upstream().await;
    let mut martin = start(&files, " auto").await;

    let plain = martin.get(&tile_path()).await;
    let steered = martin.get(&format!("{}?azimuth=90", tile_path())).await;
    assert_eq!(steered.status(), 200);
    assert_eq!(
        plain.body(),
        steered.body(),
        "a caller must not steer a server that has not opted in"
    );
    martin.stop().await;
}

#[tokio::test]
async fn request_overrides_change_the_bake_when_configured() {
    let files = upstream().await;
    let mut martin = start(&files, "\n        allow_request_overrides: true").await;

    let plain = martin.get(&tile_path()).await;
    let steered = martin.get(&format!("{}?azimuth=90", tile_path())).await;
    assert_eq!(steered.status(), 200);
    assert_images_differ(plain.body(), steered.body());
    assert_ne!(
        plain.header("etag"),
        steered.header("etag"),
        "and must change the etag, or clients keep the wrong tile"
    );
    assert_image_matches(reference("azimuth_90.png"), steered.body());
    martin.stop().await;
}

#[tokio::test]
async fn the_etag_follows_the_parameters() {
    let files = upstream().await;
    let mut martin = start(&files, "\n        allow_request_overrides: true").await;

    let baseline = etag(&martin, "").await;
    assert_eq!(baseline, etag(&martin, "").await, "the tag must be stable");
    assert_ne!(baseline, etag(&martin, "?azimuth=90").await);
    assert_ne!(baseline, etag(&martin, "?ambient=0.9").await);
    martin.stop().await;
}

async fn etag(martin: &Martin, query: &str) -> String {
    martin
        .get(&format!("{}{query}", tile_path()))
        .await
        .header("etag")
        .expect("a tile baked from identifiable inputs must carry an etag")
        .to_owned()
}

#[tokio::test]
async fn a_matching_etag_answers_not_modified() {
    let files = upstream().await;
    let mut martin = start(&files, " auto").await;

    let first = martin.get(&tile_path()).await;
    let etag = first.header("etag").expect("etag").to_owned();

    let second = martin
        .get_with_headers(&tile_path(), &[("if-none-match", &etag)])
        .await;
    assert_eq!(second.status(), 304);
    assert!(second.body().is_empty());
    martin.stop().await;
}

#[tokio::test]
async fn an_out_of_range_override_is_rejected() {
    let files = upstream().await;
    let mut martin = start(&files, "\n        allow_request_overrides: true").await;

    let response = martin.get(&format!("{}?altitude=120", tile_path())).await;
    assert_eq!(response.status(), 400);
    insta::assert_snapshot!(response.text(), @"Hillshade parameter altitude must be between `0` and `90`, but was `120`");
    martin.assert_log_contains("Hillshade parameter altitude must be between");
    martin.stop().await;
}

#[tokio::test]
async fn an_invalid_hillshade_stops_startup() {
    let files = upstream().await;
    let error = Martin::builder()
        .config(&format!(
            "passthrough:
  sources:
    terrain:
      url: {}/{{z}}/{{x}}/{{y}}
      format: png
      convert_to_hillshade:
        azimuth: 400
",
            files.base_url()
        ))
        .start()
        .await
        .expect_err("an out-of-range azimuth must stop startup");

    let StartError::EarlyExit { log, .. } = error else {
        panic!("expected an early exit, got: {error}");
    };
    assert!(
        log.contains("azimuth"),
        "the startup error must name the parameter; log:\n{log}"
    );
}
