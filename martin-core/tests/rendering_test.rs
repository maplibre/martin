#![cfg(all(feature = "rendering", target_os = "linux"))]

use std::path::{Path, PathBuf};

use martin_core::styles::StyleSources;
use rstest::rstest;
use tempfile::NamedTempFile;

// Rewrites style JSON URLs to point at the mitmproxy reverse-proxy started by
// `just with-render-cache` / `just seed-render-fixtures`. Ports must match
// the mitmdump invocation in justfile.
const PROXIED_HOSTS: &[(&str, u16)] = &[
    ("https://demotiles.maplibre.org", 18081),
    ("https://tiles.openfreemap.org", 18082),
];

fn rewrite_style(original: &Path) -> (NamedTempFile, PathBuf) {
    let body = std::fs::read_to_string(original).expect("read style");
    let mut rewritten = body;
    for (host, port) in PROXIED_HOSTS {
        rewritten = rewritten.replace(host, &format!("http://127.0.0.1:{port}"));
    }
    let tmp = NamedTempFile::with_suffix(".json").expect("create style tempfile");
    std::fs::write(tmp.path(), rewritten).expect("write rewritten style");
    let path = tmp.path().to_path_buf();
    (tmp, path)
}

#[rstest]
#[case::maplibre_demo_png("maplibre_demo.json", (0, 0, 0), image::ImageFormat::Png, "png")]
#[case::maplibre_demo_zoom1_png("maplibre_demo.json", (1, 0, 0), image::ImageFormat::Png, "png")]
#[case::maptiler_basic_png("src2/maptiler_basic.json", (0, 0, 0), image::ImageFormat::Png, "png")]
#[case::maplibre_demo_jpeg("maplibre_demo.json", (0, 0, 0), image::ImageFormat::Jpeg, "jpeg")]
#[case::maplibre_demo_zoom1_jpeg("maplibre_demo.json", (1, 0, 0), image::ImageFormat::Jpeg, "jpeg")]
#[case::maptiler_basic_jpeg("src2/maptiler_basic.json", (0, 0, 0), image::ImageFormat::Jpeg, "jpeg")]
#[tokio::test]
async fn render_tile_with_fixtures(
    #[case] style_file: &str,
    #[case] (z, x, y): (u8, u32, u32),
    #[case] format: image::ImageFormat,
    #[case] ext: &str,
) {
    let style_dir = Path::new("../tests/fixtures/styles/");
    // _tmp must outlive the render call.
    let (_tmp, style_path) = rewrite_style(&style_dir.join(style_file));
    let mut styles = StyleSources::default();
    styles.set_rendering_enabled(true);

    let rendered = styles.render(style_path, z, x, y).await.unwrap();
    let rendered_img = rendered.as_image();
    let (width, height) = (rendered_img.width(), rendered_img.height());

    assert_eq!((width, height), (512, 512), "Rendered tile must be 512x512");

    let pixels: std::collections::HashSet<_> = rendered_img.pixels().copied().collect();
    assert!(
        pixels.len() > 1,
        "Rendered image is blank (all pixels identical)"
    );

    // JPEG doesn't support alpha, so convert RGBA->RGB when needed.
    let encoded_img: image::DynamicImage = if format == image::ImageFormat::Jpeg {
        image::DynamicImage::ImageRgb8(
            image::DynamicImage::ImageRgba8(rendered_img.clone()).to_rgb8(),
        )
    } else {
        image::DynamicImage::ImageRgba8(rendered_img.clone())
    };
    let mut rendered_buf = std::io::Cursor::new(Vec::new());
    encoded_img.write_to(&mut rendered_buf, format).unwrap();
    let rendered_bytes = rendered_buf.into_inner();

    match format {
        image::ImageFormat::Png => {
            assert!(
                rendered_bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]),
                "Encoded bytes are not valid PNG (wrong magic)"
            );
        }
        image::ImageFormat::Jpeg => {
            assert!(
                rendered_bytes.starts_with(&[0xFF, 0xD8, 0xFF]),
                "Encoded bytes are not valid JPEG (wrong magic)"
            );
        }
        _ => {}
    }

    let reference_name = format!(
        "{}_{}_{}_{}.{ext}",
        style_file.replace('/', "_").replace(".json", ""),
        z,
        x,
        y
    );
    let reference_path = Path::new("../tests/fixtures/rendering_references").join(&reference_name);

    // Bless on first run; commit the file and re-run.
    let reference_bytes = std::fs::read(&reference_path).unwrap_or_else(|_| {
        std::fs::create_dir_all(reference_path.parent().unwrap()).unwrap();
        assert!(
            rendered_bytes.len() > 1000,
            "Refusing to bless suspiciously small image ({} bytes)",
            rendered_bytes.len()
        );
        std::fs::write(&reference_path, &rendered_bytes).unwrap();
        panic!(
            "Created new reference image at {reference_path:?}. Commit this file and re-run the test."
        );
    });

    let rendered_for_cmp = image::load_from_memory_with_format(&rendered_bytes, format)
        .unwrap()
        .to_rgba8();
    let reference_for_cmp = image::load_from_memory_with_format(&reference_bytes, format)
        .unwrap()
        .to_rgba8();

    let similarity = image_compare::rgba_hybrid_compare(&reference_for_cmp, &rendered_for_cmp)
        .unwrap_or_else(|e| panic!("image_compare failed: {e}"));

    // Identical = 1.0; JPEG is lossy so we accept a lower bound.
    let min_similarity = if format == image::ImageFormat::Jpeg {
        0.95
    } else {
        0.99
    };
    let score = similarity.score;
    assert!(
        score >= min_similarity,
        "Rendered image {reference_name} differs from reference: similarity score {score:.4} < {min_similarity}. \
         If this is expected, delete the existing reference file and regenerate it using the current rendering output.",
    );
}
