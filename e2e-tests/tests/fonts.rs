//! Font glyph ranges rendered from `.ttf`/`.otf` files.

use martin_e2e_tests::{Martin, StartError, TestResponse, fixture};
use pbf_font_tools::prost::Message as _;
use pbf_font_tools::{Fontstack, Glyphs};
use rstest::rstest;

const REGULAR: &str = "Overpass%20Mono%20Regular";
const LIGHT: &str = "Overpass%20Mono%20Light";

async fn martin_with_font_dir() -> Martin {
    Martin::builder()
        .arg("--font")
        .arg(fixture("fonts"))
        .start()
        .await
        .expect("failed to start martin")
}

fn fontstack(response: &TestResponse) -> Fontstack {
    Glyphs::decode(response.body())
        .expect("response body is not a glyphs protobuf")
        .stacks
        .into_iter()
        .next()
        .expect("glyphs protobuf carries no fontstack")
}

#[tokio::test]
async fn a_font_directory_is_discovered_recursively() {
    let mut martin = martin_with_font_dir().await;

    insta::assert_json_snapshot!(martin.get("/catalog").await.json()["fonts"], @r#"
    {
      "Overpass Mono Light": {
        "end": 128276,
        "family": "Overpass Mono",
        "format": "otf",
        "glyphs": 988,
        "start": 0,
        "style": "Light"
      },
      "Overpass Mono Regular": {
        "end": 128276,
        "family": "Overpass Mono",
        "format": "ttf",
        "glyphs": 988,
        "start": 0,
        "style": "Regular"
      }
    }
    "#);

    martin.stop().await;
}

#[tokio::test]
async fn a_single_font_file_publishes_only_that_font() {
    let mut martin = Martin::builder()
        .arg("--font")
        .arg(fixture("fonts/overpass-mono-regular.ttf"))
        .start()
        .await
        .expect("failed to start martin");

    let fonts = martin.get("/catalog").await.json()["fonts"].clone();
    assert!(fonts.get("Overpass Mono Regular").is_some(), "{fonts}");
    assert!(fonts.get("Overpass Mono Light").is_none(), "{fonts}");
    assert_eq!(
        martin.get(&format!("/font/{REGULAR}/0-255")).await.status(),
        200
    );
    assert_eq!(
        martin.get(&format!("/font/{LIGHT}/0-255")).await.status(),
        404
    );

    martin.stop().await;
    martin.assert_log_contains(r#"error=FontNotFound("Overpass Mono Light")"#);
    martin.assert_log_contains(r#"error="Font Overpass Mono Light not found""#);
}

#[tokio::test]
async fn a_glyph_range_is_served_as_compressed_protobuf() {
    let mut martin = martin_with_font_dir().await;

    let response = martin.get(&format!("/font/{REGULAR}/0-255")).await;
    assert_eq!(response.status(), 200);
    insta::with_settings!({filters => vec![(r"(?m)^etag: .*$", "etag: [ETAG]")]}, {
        insta::assert_snapshot!(response.headers_snapshot(), @"
        content-encoding: br
        content-type: application/x-protobuf
        etag: [ETAG]
        transfer-encoding: chunked
        vary: accept-encoding, Origin, Access-Control-Request-Method, Access-Control-Request-Headers
        ");
    });
    assert_eq!(fontstack(&response).name, "Overpass Mono Regular");

    martin.stop().await;
}

#[rstest]
#[case::first_range("0-255", 192)]
#[case::latin_extended_a("256-511", 144)]
#[case::codepoints_the_font_lacks("65280-65535", 0)]
#[tokio::test]
async fn a_glyph_range_carries_every_codepoint_the_font_covers(
    #[case] range: &str,
    #[case] glyphs: usize,
) {
    let mut martin = martin_with_font_dir().await;

    let response = martin.get(&format!("/font/{REGULAR}/{range}")).await;
    assert_eq!(response.status(), 200);
    let stack = fontstack(&response);
    assert_eq!(stack.range, range);
    assert_eq!(stack.glyphs.len(), glyphs);

    martin.stop().await;
}

#[tokio::test]
async fn a_fontstack_concatenates_the_glyphs_of_every_font() {
    let mut martin = martin_with_font_dir().await;

    let regular = fontstack(&martin.get(&format!("/font/{REGULAR}/0-255")).await);
    let light = fontstack(&martin.get(&format!("/font/{LIGHT}/0-255")).await);
    let stack = fontstack(&martin.get(&format!("/font/{REGULAR},{LIGHT}/0-255")).await);

    assert_eq!(stack.name, "Overpass Mono Regular, Overpass Mono Light");
    assert_eq!(stack.range, "0-255");
    assert_eq!(
        stack.glyphs.len(),
        regular.glyphs.len() + light.glyphs.len()
    );

    martin.stop().await;
}

#[rstest]
#[case::on_its_own("Nonexistent")]
#[case::inside_a_fontstack("Overpass%20Mono%20Regular,Nonexistent")]
#[tokio::test]
async fn an_unknown_font_is_not_found(#[case] fontstack: &str) {
    let mut martin = martin_with_font_dir().await;

    let response = martin.get(&format!("/font/{fontstack}/0-255")).await;
    assert_eq!(response.status(), 404);
    assert_eq!(response.text(), "Font Nonexistent not found");
    insta::allow_duplicates! {
        insta::assert_snapshot!(response.headers_snapshot(), @r#"
        content-encoding: br
        content-type: text/plain; charset=utf-8
        etag: W/"1a-v2HxsjSSPxQe7xjbF9rAaw=="
        transfer-encoding: chunked
        vary: accept-encoding, Origin, Access-Control-Request-Method, Access-Control-Request-Headers
        "#);
    }

    martin.stop().await;
    martin.assert_log_contains(r#"error=FontNotFound("Nonexistent")"#);
    martin.assert_log_contains(r#"error="Font Nonexistent not found""#);
}

#[rstest]
#[case::start_after_end(
    "255-0",
    "Font range start (255) must be <= end (0)",
    "error=InvalidFontRangeStartEnd { start: 255, end: 0 }"
)]
#[case::unaligned_start(
    "10-265",
    "Font range start (10) must be multiple of 256 (e.g. 0, 256, 512, ...)",
    "error=InvalidFontRangeStart(10)"
)]
#[case::unaligned_end(
    "0-100",
    "Font range end (100) must be multiple of 256 - 1 (e.g. 255, 511, 767, ...)",
    "error=InvalidFontRangeEnd(100)"
)]
#[case::span_wider_than_256(
    "0-511",
    "Given font range 0-511 is invalid. It must be 256 characters long (e.g. 0-255, 256-511, ...)",
    "error=InvalidFontRange(0, 511)"
)]
#[case::past_the_last_codepoint(
    "1114112-1114367",
    "Font range start (1114112) must be <= end (1114367)",
    "error=InvalidFontRangeStartEnd { start: 1114112, end: 1114367 }"
)]
#[tokio::test]
async fn a_range_outside_the_256_codepoint_grid_is_rejected(
    #[case] range: &str,
    #[case] message: &str,
    #[case] logged: &str,
) {
    let mut martin = martin_with_font_dir().await;

    let response = martin.get(&format!("/font/{REGULAR}/{range}")).await;
    assert_eq!(response.status(), 400);
    assert_eq!(response.text(), message);

    martin.stop().await;
    martin.assert_log_contains(logged);
    martin.assert_log_contains(&format!("error={message:?}"));
}

#[tokio::test]
async fn a_glyph_range_answers_conditional_requests() {
    let mut martin = martin_with_font_dir().await;

    let path = format!("/font/{REGULAR}/0-255");
    let first = martin.get(&path).await;
    let etag = first
        .header("etag")
        .expect("a glyph range must carry an etag")
        .to_owned();

    let cached = martin
        .get_with_headers(&path, &[("if-none-match", &etag)])
        .await;
    assert_eq!(cached.status(), 304);
    assert!(cached.body().is_empty());

    let stale = martin
        .get_with_headers(
            &path,
            &[("if-none-match", r#"W/"0-0000000000000000000000""#)],
        )
        .await;
    assert_eq!(stale.status(), 200);
    assert_eq!(stale.body(), first.body());

    martin.stop().await;
}

#[tokio::test]
async fn the_plural_fonts_path_redirects() {
    let mut martin = martin_with_font_dir().await;

    let path = format!("/fonts/{REGULAR}/0-255");
    for response in [martin.get(&path).await, martin.head(&path).await] {
        assert_eq!(response.status(), 301);
        insta::allow_duplicates! {
            insta::assert_snapshot!(response.headers_snapshot(), @"
            content-length: 0
            location: /font/Overpass Mono Regular/0-255
            vary: Origin, Access-Control-Request-Method, Access-Control-Request-Headers
            ");
        }
    }
    assert_eq!(
        martin.get(&format!("/font/{REGULAR}/0-255")).await.status(),
        200
    );

    martin.stop().await;
}

#[rstest]
#[case::singular("font")]
#[case::plural("fonts")]
#[tokio::test]
async fn a_glyph_url_with_a_file_extension_redirects(#[case] segment: &str) {
    let mut martin = martin_with_font_dir().await;

    let path = format!("/{segment}/{REGULAR}/0-255.pbf");
    for response in [martin.get(&path).await, martin.head(&path).await] {
        assert_eq!(response.status(), 301);
        insta::allow_duplicates! {
            insta::assert_snapshot!(response.headers_snapshot(), @"
            content-length: 0
            location: /font/Overpass Mono Regular/0-255
            vary: Origin, Access-Control-Request-Method, Access-Control-Request-Headers
            ");
        }
    }
    assert_eq!(
        martin.get(&format!("/font/{REGULAR}/0-255")).await.status(),
        200
    );

    martin.stop().await;
}

#[tokio::test]
async fn a_font_configured_from_two_paths_is_registered_once() {
    let mut martin = Martin::builder()
        .arg("--font")
        .arg(fixture("fonts/overpass-mono-regular.ttf"))
        .arg("--font")
        .arg(fixture("fonts"))
        .start()
        .await
        .expect("failed to start martin");

    let fonts = martin.get("/catalog").await.json()["fonts"].clone();
    assert_eq!(
        fonts.as_object().map(serde_json::Map::len),
        Some(2),
        "{fonts}"
    );
    assert_eq!(
        martin.get(&format!("/font/{REGULAR}/0-255")).await.status(),
        200
    );

    martin.stop().await;
    martin.assert_log_contains(
        "Ignoring duplicate font: already configured from another path font.name=Overpass Mono Regular",
    );
}

#[tokio::test]
async fn an_alias_serves_the_fonts_it_combines() {
    let mut martin = Martin::builder()
        .config(
            "
fonts:
  paths: tests/fixtures/fonts
  aliases:
    Overpass Mono: [Overpass Mono Regular, Overpass Mono Light]
",
        )
        .start()
        .await
        .expect("failed to start martin");

    insta::assert_json_snapshot!(martin.get("/catalog").await.json()["fonts"], @r#"
    {
      "Overpass Mono": {
        "end": 128276,
        "family": "Overpass Mono",
        "glyphs": 934,
        "start": 0
      },
      "Overpass Mono Light": {
        "end": 128276,
        "family": "Overpass Mono",
        "format": "otf",
        "glyphs": 988,
        "start": 0,
        "style": "Light"
      },
      "Overpass Mono Regular": {
        "end": 128276,
        "family": "Overpass Mono",
        "format": "ttf",
        "glyphs": 988,
        "start": 0,
        "style": "Regular"
      }
    }
    "#);

    let aliased = martin.get("/font/Overpass%20Mono/0-255").await;
    assert_eq!(aliased.status(), 200);
    let explicit = martin.get(&format!("/font/{REGULAR},{LIGHT}/0-255")).await;
    assert_eq!(aliased.body(), explicit.body());
    let stack = fontstack(&aliased);
    assert_eq!(stack.name, "Overpass Mono Regular, Overpass Mono Light");

    martin.stop().await;
    martin.assert_log_contains("Configured font alias");
}

#[tokio::test]
async fn an_alias_may_shadow_the_font_it_extends() {
    let mut martin = Martin::builder()
        .config(
            "
fonts:
  paths: tests/fixtures/fonts
  aliases:
    Overpass Mono Regular: [Overpass Mono Regular, Overpass Mono Light]
",
        )
        .start()
        .await
        .expect("failed to start martin");

    let response = martin.get(&format!("/font/{REGULAR}/0-255")).await;
    assert_eq!(response.status(), 200);
    let stack = fontstack(&response);
    assert_eq!(stack.name, "Overpass Mono Regular, Overpass Mono Light");

    martin.stop().await;
    martin.assert_log_contains(
        "Font alias shadows a font of the same name; requests for it will serve the alias",
    );
}

#[tokio::test]
async fn an_alias_naming_an_unknown_font_fails_startup() {
    let error = Martin::builder()
        .config(
            "
fonts:
  paths: tests/fixtures/fonts
  aliases:
    Overpass Mono: [Overpass Mono Regular, Nonexistent]
",
        )
        .start()
        .await
        .expect_err("martin must reject an alias naming an unknown font");
    let StartError::EarlyExit { status, log } = error else {
        panic!("expected an early exit, got: {error}");
    };
    assert!(!status.success(), "exit status must be a failure: {status}");
    assert!(
        log.contains(r#"Font alias "Overpass Mono" references unknown font "Nonexistent""#),
        "log must name the alias and the missing font; log:\n{log}"
    );
}
