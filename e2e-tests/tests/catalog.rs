//! The `/catalog` document every source kind contributes to.

use martin_e2e_tests::{Martin, TestResponse, fixture};

async fn martin_with_every_source_kind() -> Martin {
    Martin::builder()
        .arg("tests/fixtures/geojson")
        .arg("--sprite")
        .arg(fixture("sprites/src1"))
        .arg("--sprite")
        .arg(fixture("sprites/src2"))
        .arg("--font")
        .arg(fixture("fonts"))
        .arg("--style")
        .arg(fixture("styles/maplibre_demo.json"))
        .arg("--style")
        .arg(fixture("styles/src2"))
        .start()
        .await
        .expect("failed to start martin")
}

/// Assert every catalog section holds sources and lists its keys in sorted order.
/// Parsing sorts the keys, so the on-the-wire order is checked against the raw body.
fn assert_sorted_and_populated(catalog: &TestResponse) {
    let parsed = catalog.json();
    let body = catalog.text();
    for section in ["tiles", "sprites", "fonts", "styles"] {
        let entries = parsed[section]
            .as_object()
            .expect("a catalog section is an object");
        assert!(
            !entries.is_empty(),
            "the `{section}` section has no sources"
        );
        let offsets = entries
            .keys()
            .map(|key| {
                body.find(&format!("\"{key}\":"))
                    .expect("the body names every key it was parsed from")
            })
            .collect::<Vec<_>>();
        assert!(
            offsets.is_sorted(),
            "the `{section}` keys are not sorted in {body}"
        );
    }
}

/// Two servers over the same sources answer a byte-identical catalog.
#[tokio::test]
async fn two_servers_over_the_same_sources_answer_the_same_bytes() {
    let mut first = martin_with_every_source_kind().await;
    let mut second = martin_with_every_source_kind().await;

    let first_catalog = first.get("/catalog").await;
    let second_catalog = second.get("/catalog").await;
    assert_eq!(first_catalog.status(), 200);
    assert_eq!(second_catalog.status(), 200);
    assert_sorted_and_populated(&first_catalog);
    assert_sorted_and_populated(&second_catalog);
    assert_eq!(first_catalog.text(), second_catalog.text());

    for martin in [&mut first, &mut second] {
        martin.stop().await;
        martin.assert_startup_warnings();
        martin.assert_log_clean();
    }
}
