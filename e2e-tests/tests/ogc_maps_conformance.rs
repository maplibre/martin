//! The OGC API - Maps conformance suite, run by its own test engine against a rendered `maplibre_demo` style.
#![cfg(all(feature = "test-ogc-maps", target_os = "linux"))]

use indoc::formatdoc;
use martin_e2e_tests::{Cassette, Martin, TeamEngine, fixture, mbtiles_fixture, temp_dir};

/// The test engine that `just test-ogc-maps` starts from docker compose.
const TEAM_ENGINE: &str = "http://127.0.0.1:8081/teamengine";

#[tokio::test]
async fn the_ogc_api_passes_the_maps_conformance_suite() {
    let cassette = Cassette::serving(&["demotiles.maplibre.org"]).await;
    let maplibre_demo = cassette.style(fixture("styles/maplibre_demo.json"));
    let maplibre_demo = maplibre_demo.display();
    let dir = temp_dir();
    let world_cities = mbtiles_fixture(dir.path(), "world_cities").await;
    let world_cities = world_cities.display();
    let mut martin = Martin::builder()
        .config(&formatdoc! {"
            mbtiles:
              sources:
                world_cities: {world_cities}
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
    insta::assert_snapshot!(martin.redact(&report), @"
    FAIL verifyCollectionDescriptionCrs: Collection description for 'world_cities' must include a 'crs' array. expected object to not be null
    FAIL verifyCollectionDescriptionLinks: Collection description for 'world_cities' must include a map link with rel 'https://www.opengis.net/def/rel/ogc/1.0/map' or 'ogc-rel:map'. expected object to not be null
    FAIL verifyCollectionsParameter: A.11 verifyCollectionsParameter failed:
    [Req11/param-defined] The OpenAPI document does not declare 'collections' as an optional query parameter for the map operation path.
    [Req11/single-id] Expected HTTP 200 for collections='world_cities' but got HTTP 404.
    [Req11/full-url-id] Expected HTTP 200 for full URL form collections='http://[ADDR]/ogc/collections/world_cities' but got HTTP 404.
    FAIL verifyDatasetMapLinkFromLandingPage: Landing page must include a dataset map link with rel 'https://www.opengis.net/def/rel/ogc/1.0/map', legacy rel 'http://www.opengis.net/def/rel/ogc/1.0/map', or compact rel 'ogc-rel:map'. expected object to not be null
    FAIL verifyLandingPageExtentCrs: Landing page JSON response must include an 'extent' object. expected [true] but found [false]
    FAIL verifyMapConformanceSuccess: The required conformance class 'conf/core' or legacy 'req/core' is not declared. expected [true] but found [false]
    FAIL verifyMapResponse: Test Skipped: No map resources found in the collections to verify. Please check the target server. expected [true] but found [false]
    FAIL verifyStyledMapLinks: Styled map link verification failed with 1 error(s):
      1. Style resource 'maplibre_demo' in 'http://[ADDR]/ogc/styles' does not include a map link with rel 'https://www.opengis.net/def/rel/ogc/1.0/map'.
    FAIL verifyTilesetsLink: No link with relation 'https://www.opengis.net/def/rel/ogc/1.0/tilesets-map' was found in the Landing Page or any Collection. The implementation must provide a link to the Map Tilesets if it claims to support the Map Tilesets conformance class. expected [true] but found [false]
    PASS verifyMapRetrievalOperation
    SKIP verifyApiOperationIdentifiers: Server does not declare conformance class https://www.opengis.net/spec/ogcapi-maps-1/1.0/conf/api-operations. Skipping A.62 tests.
    SKIP verifyApiOperationsCompleteness: Server does not declare conformance class http://www.opengis.net/spec/ogcapi-maps-1/1.0/conf/api-operations. Skipping A.61 tests.
    SKIP verifyBackgroundMapSuccess: Test Skipped: SUT does not declare conformance to: /conf/background
    SKIP verifyBgcolorDefinition: Test Skipped: SUT does not declare conformance to /conf/background
    SKIP verifyCollectionMapOperation: No collection descriptions include a map link to test Requirement /req/collection-map/map-operation.
    SKIP verifyCorsSupport: No map-related resources found to verify CORS support.
    SKIP verifyJpegContent: No map resource URL found. The server must provide a link with rel='[ogc-rel:map]' in the landing page or collection descriptions.
    SKIP verifyPngContent: No map resource URL found. The server must provide a link with rel='[ogc-rel:map]' in the landing page or collection descriptions.
    SKIP verifyStyledMapOperation: No styled map links were found in any style resource.
    SKIP verifyTiffContent: No map resource URL found. The server must provide a link with rel='[ogc-rel:map]' in the landing page or collection descriptions.
    SKIP verifyTilesParameters: No tileset URL available for testing tiles-parameters.
    SKIP verifyTransparentDefinition: Test Skipped: SUT does not declare conformance to: /conf/background
    SKIP verifyVoidColorDefinition: Test Skipped: SUT does not declare conformance to: /conf/background
    SKIP verifyVoidTransparentDefinition: Test Skipped: SUT does not declare conformance to: /conf/background
    ");
    martin.stop().await;
    martin.take_log_lines("[Render]");
    cassette.assert_no_misses();
}
