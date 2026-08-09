//! Spritesheets generated from directories of SVG files.

use martin_integration_tests::{Martin, fixture};
use rstest::rstest;

async fn martin_with_sprite_dirs() -> Martin {
    Martin::builder()
        .arg("--sprite")
        .arg(fixture("sprites/src1"))
        .arg("--sprite")
        .arg(fixture("sprites/src2"))
        .start()
        .await
        .expect("failed to start martin")
}

#[tokio::test]
async fn a_sprite_directory_is_discovered_recursively() {
    let mut martin = martin_with_sprite_dirs().await;

    insta::assert_json_snapshot!(martin.get("/catalog").await.json()["sprites"], @r#"
    {
      "src1": {
        "images": [
          "another_bicycle",
          "bear",
          "sub/circle"
        ]
      },
      "src2": {
        "images": [
          "bicycle"
        ]
      }
    }
    "#);

    martin.stop().await;
    martin.assert_log_clean();
}

#[tokio::test]
async fn an_index_places_every_icon_on_the_sheet() {
    let mut martin = martin_with_sprite_dirs().await;

    let response = martin.get("/sprite/src1.json").await;
    assert_eq!(response.status(), 200);
    insta::assert_snapshot!(response.headers_snapshot(), @r#"
    content-encoding: br
    content-type: application/json
    etag: W/"c6-hm38ksFM7OrTdjwShP2iRA=="
    transfer-encoding: chunked
    vary: accept-encoding, Origin, Access-Control-Request-Method, Access-Control-Request-Headers
    "#);
    insta::assert_json_snapshot!(response.json(), @r#"
    {
      "another_bicycle": {
        "height": 15,
        "pixelRatio": 1,
        "width": 15,
        "x": 20,
        "y": 16
      },
      "bear": {
        "height": 16,
        "pixelRatio": 1,
        "width": 16,
        "x": 20,
        "y": 0
      },
      "sub/circle": {
        "height": 20,
        "pixelRatio": 1,
        "width": 20,
        "x": 0,
        "y": 0
      }
    }
    "#);

    martin.stop().await;
    martin.assert_log_clean();
}

#[tokio::test]
async fn a_sheet_is_served_as_png() {
    let mut martin = martin_with_sprite_dirs().await;

    let response = martin.get("/sprite/src1.png").await;
    assert_eq!(response.status(), 200);
    insta::assert_snapshot!(response.headers_snapshot(), @r#"
    content-length: 758
    content-type: image/png
    etag: W/"2f6-PZaBSgIOiVTrPDnRiX0hYg=="
    vary: Origin, Access-Control-Request-Method, Access-Control-Request-Headers
    "#);

    martin.stop().await;
    martin.assert_log_clean();
}

#[rstest]
#[case::one_source("/sprite/src1.png", (36, 31))]
#[case::another_source("/sprite/src2.png", (15, 15))]
#[case::composite("/sprite/src1,src2.png", (50, 31))]
#[case::high_dpi("/sprite/src1@2x.png", (72, 62))]
#[case::sdf("/sdf_sprite/src1.png", (48, 43))]
#[case::sdf_composite("/sdf_sprite/src1,src2.png", (48, 47))]
#[tokio::test]
async fn a_sheet_is_packed_to_fit_every_icon(#[case] path: &str, #[case] size: (u32, u32)) {
    let mut martin = martin_with_sprite_dirs().await;

    let response = martin.get(path).await;
    assert_eq!(response.status(), 200);
    assert_eq!(response.image_size(), size);

    martin.stop().await;
    martin.assert_log_clean();
}

#[tokio::test]
async fn a_2x_index_doubles_every_dimension() {
    let mut martin = martin_with_sprite_dirs().await;

    let single = martin.get("/sprite/src1.json").await.json();
    let double = martin.get("/sprite/src1@2x.json").await.json();

    for (name, icon) in single.as_object().expect("the index is a json object") {
        assert_eq!(icon["pixelRatio"], 1, "{name}");
        assert_eq!(double[name]["pixelRatio"], 2, "{name}");
        for field in ["width", "height", "x", "y"] {
            assert_eq!(
                double[name][field].as_i64(),
                icon[field].as_i64().map(|value| value * 2),
                "{name}.{field}"
            );
        }
    }

    martin.stop().await;
    martin.assert_log_clean();
}

#[tokio::test]
async fn an_sdf_index_flags_every_icon() {
    let mut martin = martin_with_sprite_dirs().await;

    let response = martin.get("/sdf_sprite/src1.json").await;
    assert_eq!(response.status(), 200);
    insta::assert_json_snapshot!(response.json(), @r#"
    {
      "another_bicycle": {
        "height": 21,
        "pixelRatio": 1,
        "sdf": true,
        "width": 21,
        "x": 26,
        "y": 22
      },
      "bear": {
        "height": 22,
        "pixelRatio": 1,
        "sdf": true,
        "width": 22,
        "x": 26,
        "y": 0
      },
      "sub/circle": {
        "height": 26,
        "pixelRatio": 1,
        "sdf": true,
        "width": 26,
        "x": 0,
        "y": 0
      }
    }
    "#);

    for icon in martin
        .get("/sprite/src1.json")
        .await
        .json()
        .as_object()
        .expect("the index is a json object")
        .values()
    {
        assert_eq!(icon.get("sdf"), None, "{icon}");
    }

    martin.stop().await;
    martin.assert_log_clean();
}

#[tokio::test]
async fn a_composite_index_merges_every_source() {
    let mut martin = martin_with_sprite_dirs().await;

    let index = martin.get("/sprite/src1,src2.json").await.json();
    let names = index
        .as_object()
        .expect("the index is a json object")
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(names, ["another_bicycle", "bear", "bicycle", "sub/circle"]);

    martin.stop().await;
    martin.assert_log_clean();
}

#[rstest]
#[case::index("/sprite/nope.json")]
#[case::sheet("/sprite/nope.png")]
#[case::sdf_index("/sdf_sprite/nope.json")]
#[case::sdf_sheet("/sdf_sprite/nope.png")]
#[tokio::test]
async fn an_unknown_sprite_source_is_not_found(#[case] path: &str) {
    let mut martin = martin_with_sprite_dirs().await;

    let response = martin.get(path).await;
    assert_eq!(response.status(), 404);
    assert_eq!(response.text(), "Sprite nope not found");

    martin.stop().await;
    martin.assert_log_contains(r#"error=SpriteNotFound("nope")"#);
    martin.assert_log_contains(r#"error="Sprite nope not found""#);
    martin.assert_log_clean();
}

#[rstest]
#[case::index("/sprites/src1.json", "/sprite/src1.json")]
#[case::sheet("/sprites/src1.png", "/sprite/src1.png")]
#[case::sdf_index("/sdf_sprites/src1.json", "/sdf_sprite/src1.json")]
#[case::sdf_sheet("/sdf_sprites/src1.png", "/sdf_sprite/src1.png")]
#[tokio::test]
async fn the_plural_sprites_path_redirects(#[case] path: &str, #[case] location: &str) {
    let mut martin = martin_with_sprite_dirs().await;

    for response in [martin.get(path).await, martin.head(path).await] {
        assert_eq!(response.status(), 301);
        assert_eq!(response.header("location"), Some(location));
    }
    assert_eq!(martin.get(location).await.status(), 200);

    martin.stop().await;
    martin.assert_log_clean();
}
