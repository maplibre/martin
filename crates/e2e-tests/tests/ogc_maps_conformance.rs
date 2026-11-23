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
    insta::assert_snapshot!(martin.redact(&report), @"
    PASS verifyCollectionDescriptionCrs
    PASS verifyCollectionDescriptionLinks
    PASS verifyCollectionMapOperation
    PASS verifyCollectionsParameter
    PASS verifyCorsSupport
    PASS verifyDatasetMapLinkFromLandingPage
    PASS verifyJpegContent
    PASS verifyLandingPageExtentCrs
    PASS verifyMapConformanceSuccess
    PASS verifyMapResponse
    PASS verifyMapRetrievalOperation
    PASS verifyPngContent
    PASS verifyStyledMapLinks
    PASS verifyStyledMapOperation
    PASS verifyTiffContent
    PASS verifyTilesParameters
    PASS verifyTilesetsLink
    SKIP verifyApiOperationIdentifiers: Server does not declare conformance class https://www.opengis.net/spec/ogcapi-maps-1/1.0/conf/api-operations. Skipping A.62 tests.
    SKIP verifyApiOperationsCompleteness: Server does not declare conformance class http://www.opengis.net/spec/ogcapi-maps-1/1.0/conf/api-operations. Skipping A.61 tests.
    SKIP verifyBackgroundMapSuccess: Test Skipped: SUT does not declare conformance to: /conf/background
    SKIP verifyBgcolorDefinition: Test Skipped: SUT does not declare conformance to /conf/background
    SKIP verifyTransparentDefinition: Test Skipped: SUT does not declare conformance to: /conf/background
    SKIP verifyVoidColorDefinition: Test Skipped: SUT does not declare conformance to: /conf/background
    SKIP verifyVoidTransparentDefinition: Test Skipped: SUT does not declare conformance to: /conf/background
    ");
    martin.stop().await;
    martin.take_log_lines("[Render]");
    cassette.assert_no_misses();
}
