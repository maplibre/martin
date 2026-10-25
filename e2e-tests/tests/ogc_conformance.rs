//! The OGC API - Tiles conformance suite, run by its own test engine against a served `world_cities`.
#![cfg(feature = "test-ogc")]

use std::time::Duration;

use martin_e2e_tests::{Martin, mbtiles_fixture, temp_dir};

/// The test engine that `just test-ogc` starts from docker compose.
const TEAM_ENGINE: &str = "http://127.0.0.1:8080/teamengine";

#[tokio::test]
async fn the_ogc_api_passes_the_tiles_conformance_suite() {
    let dir = temp_dir();
    let mbtiles = mbtiles_fixture(dir.path(), "world_cities").await;
    let mut martin = Martin::builder().arg(&mbtiles).start().await.expect("martin starts");
    let client = reqwest::Client::new();
    wait_for_team_engine(&client).await;

    let iut = format!("http://{}/ogc", martin.addr());
    let tiles = format!("{iut}/collections/world_cities/tiles/WebMercatorQuad/{{tileMatrix}}/{{tileRow}}/{{tileCol}}");
    let run = reqwest::Url::parse_with_params(
        &format!("{TEAM_ENGINE}/rest/suites/ogcapi-tiles-1.0/run"),
        [
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
    .expect("a suite URL");
    let report = client
        .get(run)
        .basic_auth("ogctest", Some("ogctest"))
        .header("Accept", "application/xml")
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .expect("the suite runs")
        .text()
        .await
        .expect("the suite reports");
    insta::assert_snapshot!(martin.redact(&outcomes(&report)), @"
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
    SKIP apiDefinitionOperationIdValidation: Skipped because the API definition could not be retrieved
    SKIP apiDefinitionValidation: Skipped because the API definition could not be retrieved
    SKIP landingPageRetrieval: Skipped because Conformance Class http://www.opengis.net/spec/ogcapi-tiles-1/1.0/conf/dataset-tilesets is not implemented.
    ");
    martin.stop().await;
    martin.assert_log_contains(
        r#"ERROR error="Tile 2/2147483647/2147483647 is outside the WebMercatorQuad grid of world_cities""#,
    );
}

/// Waits up to five minutes for the test engine to answer.
async fn wait_for_team_engine(client: &reqwest::Client) {
    let mut answered = false;
    for _ in 0..150 {
        let response = client.get(format!("{TEAM_ENGINE}/")).send().await;
        answered = response.is_ok_and(|response| response.status().is_success());
        if answered {
            break;
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    assert!(answered, "the test engine did not answer at {TEAM_ENGINE}");
}

/// One line per test in the report, with its status and the reason for anything but a pass.
fn outcomes(report: &str) -> String {
    let report = roxmltree::Document::parse(report).expect("a TestNG report");
    let mut lines: Vec<String> = report
        .descendants()
        .filter(|node| node.has_tag_name("test-method") && node.attribute("is-config") != Some("true"))
        .map(|test| {
            let status = test.attribute("status").expect("a status");
            let name = test.attribute("name").expect("a name");
            let reason = test
                .descendants()
                .find(|node| node.has_tag_name("message"))
                .and_then(|message| message.text())
                .map(str::trim);
            match (status, reason) {
                ("PASS", _) | (_, None) => format!("{status} {name}"),
                (_, Some(reason)) => format!("{status} {name}: {reason}"),
            }
        })
        .collect();
    lines.sort();
    lines.join("\n")
}
