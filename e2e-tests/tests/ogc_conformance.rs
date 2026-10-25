//! The OGC API - Tiles conformance suite, run by its own test engine against a served `world_cities`.
#![cfg(feature = "test-ogc")]

use martin_e2e_tests::{Martin, TeamEngine, mbtiles_fixture, temp_dir};

/// The test engine that `just test-ogc` starts from docker compose.
const TEAM_ENGINE: &str = "http://127.0.0.1:8080/teamengine";

#[tokio::test]
async fn the_ogc_api_passes_the_tiles_conformance_suite() {
    let dir = temp_dir();
    let mbtiles = mbtiles_fixture(dir.path(), "world_cities").await;
    let mut martin = Martin::builder().arg(&mbtiles).start().await.expect("martin starts");
    let team_engine = TeamEngine::ready(TEAM_ENGINE).await;

    let iut = format!("http://{}/ogc", martin.addr());
    let tiles = format!("{iut}/collections/world_cities/tiles/WebMercatorQuad/{{tileMatrix}}/{{tileRow}}/{{tileCol}}");
    let report = team_engine
        .run(
            "ogcapi-tiles-1.0",
            &[
                ("iut", iut.as_str()),
                (
                    "tilematrixsetdefinitionuri",
                    "http://www.opengis.net/def/tilematrixset/OGC/1.0/WebMercatorQuad",
                ),
                ("urltemplatefortiles", tiles.as_str()),
                ("tilematrix", "2"),
                ("mintilerow", "1"),
                ("maxtilerow", "2"),
                ("mintilecol", "3"),
                ("maxtilecol", "3"),
            ],
        )
        .await;
    insta::assert_snapshot!(martin.redact(&report), @"
    PASS apiDefinitionOperationIdValidation
    PASS apiDefinitionValidation
    PASS geospatialResourceTilesetsLinksCheck
    PASS geospatialResourceTilesetsRetrieval
    PASS validateConformanceOperationAndResponse
    PASS validateSuccessfulTilesExecution
    PASS validateSuccessfulTilesExecutionFollowingLinks
    PASS validateTileColDefinitionIsAvailable
    PASS validateTileMatrixDefinitionIsAvailable
    PASS validateTileRowDefinitionIsAvailable
    PASS validateTilesAreAvailable
    PASS validateTilesErrorConditions
    PASS validateTilesetsListResponse
    PASS verifyErrorResponses
    PASS verifyMinimalConformance
    SKIP landingPageRetrieval: Skipped because Conformance Class http://www.opengis.net/spec/ogcapi-tiles-1/1.0/conf/dataset-tilesets is not implemented.
    ");
    martin.stop().await;
    martin.assert_log_contains(
        r#"ERROR error="Tile 2/2147483647/2147483647 is outside the WebMercatorQuad grid of world_cities""#,
    );
}
