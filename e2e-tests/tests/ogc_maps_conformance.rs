//! The OGC API - Maps conformance suite, run by its own test engine against a rendered `maplibre_demo` style
//! and the raster collections `png` and `toner`, as only rasters have collection maps.
#![cfg(all(feature = "test-ogc-maps", target_os = "linux"))]

use indoc::formatdoc;
use martin_e2e_tests::{Cassette, Martin, TeamEngine, fixture};

/// The test engine that `just test-ogc-maps` starts from docker compose.
const TEAM_ENGINE: &str = "http://127.0.0.1:8081/teamengine";

#[tokio::test]
async fn the_ogc_api_passes_the_maps_conformance_suite() {
    let cassette = Cassette::serving(&["demotiles.maplibre.org"]).await;
    let maplibre_demo = cassette.style(fixture("styles/maplibre_demo.json"));
    let maplibre_demo = maplibre_demo.display();
    let png = fixture("pmtiles/png.pmtiles");
    let png = png.display();
    let toner = fixture("pmtiles/stamen_toner__raster_CC-BY+ODbL_z3.pmtiles");
    let toner = toner.display();
    let mut martin = Martin::builder()
        .config(&formatdoc! {"
            pmtiles:
              sources:
                png: {png}
                toner: {toner}
            styles:
              rendering:
                enabled: true
                workers: 2
              sources:
                maplibre_demo: {maplibre_demo}
        "})
        .start()
        .await
        .expect("martin starts");
    let team_engine = TeamEngine::ready(TEAM_ENGINE).await;

    let iut = format!("http://{}/ogc", martin.addr());
    let report = team_engine
        .run(
            "ogcapi-maps-1.0",
            &[("iut", iut.as_str()), ("tile_matrix_set", "WebMercatorQuad")],
        )
        .await;
    insta::assert_snapshot!(martin.redact(&report), @r#"
    FAIL verifyCollectionsParameter: A.11 verifyCollectionsParameter failed:
    [Req11/param-defined] The OpenAPI document does not declare 'collections' as an optional query parameter for the map operation path.
    [Req11/single-id] Expected HTTP 200 for collections='png' but got HTTP 404.
    [Req11/multi-id] Expected HTTP 200 for collections='png,toner' but got HTTP 404.
    [Req11/full-url-id] Expected HTTP 200 for full URL form collections='http://[ADDR]/ogc/collections/png' but got HTTP 404.
    FAIL verifyCorsSupport: CORS verification failed with 4 error(s):
      1. GET collection-map resource 'http://[ADDR]/ogc/collections/png/map' did not return a valid Access-Control-Allow-Origin header for origin https://example.com. Actual value: null
      2. GET collection-tilesets-map resource 'http://[ADDR]/ogc/collections/png/tiles' did not return a valid Access-Control-Allow-Origin header for origin https://example.com. Actual value: null
      3. GET collection-map resource 'http://[ADDR]/ogc/collections/toner/map' did not return a valid Access-Control-Allow-Origin header for origin https://example.com. Actual value: null
      4. GET collection-tilesets-map resource 'http://[ADDR]/ogc/collections/toner/tiles' did not return a valid Access-Control-Allow-Origin header for origin https://example.com. Actual value: null
    FAIL verifyDatasetMapLinkFromLandingPage: Landing page must include a dataset map link with rel 'https://www.opengis.net/def/rel/ogc/1.0/map', legacy rel 'http://www.opengis.net/def/rel/ogc/1.0/map', or compact rel 'ogc-rel:map'. expected object to not be null
    FAIL verifyLandingPageExtentCrs: Landing page JSON response must include an 'extent' object. expected [true] but found [false]
    FAIL verifyMapResponse: Test Skipped: No map resources found in the collections to verify. Please check the target server. expected [true] but found [false]
    FAIL verifyMapRetrievalOperation: Cannot invoke "java.util.List.iterator()" because "contentTypeList" is null
    FAIL verifyPngContent: PNG content verification failed with 1 error(s):
      1. [Part C] PNG image does not support alpha channel. The PNG format SHALL support alpha channel for partial transparency when required
    FAIL verifyTiffContent: TIFF content verification failed with 2 error(s):
      1. [Part A] Expected Content-Type containing 'image/tiff' but got 'image/png'
      2. [Part A] Response body does not have a valid TIFF file signature
    PASS verifyCollectionDescriptionCrs
    PASS verifyCollectionDescriptionLinks
    PASS verifyCollectionMapOperation
    PASS verifyJpegContent
    PASS verifyMapConformanceSuccess
    PASS verifyStyledMapLinks
    PASS verifyStyledMapOperation
    PASS verifyTilesParameters
    PASS verifyTilesetsLink
    SKIP verifyApiOperationIdentifiers: Server does not declare conformance class https://www.opengis.net/spec/ogcapi-maps-1/1.0/conf/api-operations. Skipping A.62 tests.
    SKIP verifyApiOperationsCompleteness: Server does not declare conformance class http://www.opengis.net/spec/ogcapi-maps-1/1.0/conf/api-operations. Skipping A.61 tests.
    SKIP verifyBackgroundMapSuccess: Test Skipped: SUT does not declare conformance to: /conf/background
    SKIP verifyBgcolorDefinition: Test Skipped: SUT does not declare conformance to /conf/background
    SKIP verifyTransparentDefinition: Test Skipped: SUT does not declare conformance to: /conf/background
    SKIP verifyVoidColorDefinition: Test Skipped: SUT does not declare conformance to: /conf/background
    SKIP verifyVoidTransparentDefinition: Test Skipped: SUT does not declare conformance to: /conf/background
    "#);
    martin.stop().await;
    martin.take_log_lines("[Render]");
    cassette.assert_no_misses();
}
