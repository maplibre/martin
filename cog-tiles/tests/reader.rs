#![allow(clippy::unwrap_used)]

mod common;

use std::sync::Arc;
use std::sync::atomic::Ordering;

use cog_tiles::{AsyncCogReader, CogError, MemoryBackend, SkippedImage, TileCoord, TileFormat};
use common::*;
use rstest::rstest;

async fn open(name: &str) -> AsyncCogReader<MemoryBackend> {
    AsyncCogReader::try_from_backend(memory(name))
        .await
        .unwrap()
}

async fn open_err(name: &str) -> String {
    AsyncCogReader::try_from_backend(memory(name))
        .await
        .expect_err("the fixture should be rejected")
        .to_string()
}

fn z16(col: u32, row: u32) -> TileCoord {
    TileCoord::new(16, Z15_X * 2 + col, Z15_Y * 2 + row).unwrap()
}

#[tokio::test]
async fn jpeg_tiles_pass_through_as_standalone_images() {
    let reader = open("rgb_jpeg.tif").await;
    let info = reader.info();
    assert_eq!(info.min_zoom, 15);
    assert_eq!(info.max_zoom, 16);
    assert_eq!(info.tile_size, 256);
    assert_eq!(info.format, TileFormat::Jpeg);
    let [west, south, east, north] = info.bounds;
    assert!((west - -0.109_863_281_25).abs() < 1e-9, "{west}");
    assert!(west < east && south < north);

    for (col, row) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
        let tile = reader.get_tile(z16(col, row)).await.unwrap().unwrap();
        assert_eq!(&tile[..2], &[0xFF, 0xD8]);
        let (w, h, pixels) = decode_jpeg(&tile);
        assert_eq!(w, 256);
        assert_eq!(h, 256);
        let diff = mean_abs_diff(&pixels, &expected_tile(col * 256, row * 256, 3, 0));
        assert!(diff < 4.0, "tile {col},{row} differs by {diff}");
    }
    let overview = reader
        .get_tile(TileCoord::new(15, Z15_X, Z15_Y).unwrap())
        .await
        .unwrap();
    assert_eq!(decode_jpeg(&overview.unwrap()).0, 256);
}

#[tokio::test]
async fn missing_tiles_are_none() {
    let reader = open("rgb_jpeg.tif").await;
    // Zooms the file has no image for.
    assert!(
        reader
            .get_tile(TileCoord::new(14, Z15_X / 2, Z15_Y / 2).unwrap())
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        reader
            .get_tile(TileCoord::new(17, Z15_X * 4, Z15_Y * 4).unwrap())
            .await
            .unwrap()
            .is_none()
    );
    // Right zoom, outside the image.
    assert!(reader.get_tile(z16(2, 0)).await.unwrap().is_none());
    assert!(
        reader
            .get_tile(TileCoord::new(16, Z15_X * 2 - 1, Z15_Y * 2).unwrap())
            .await
            .unwrap()
            .is_none()
    );
    assert!(reader.contains(z16(1, 1)));
    assert!(!reader.contains(z16(2, 1)));
}

#[tokio::test]
async fn webp_tiles_pass_through() {
    let reader = open("rgb_webp.tif").await;
    assert_eq!(reader.info().format, TileFormat::Webp);
    let tile = reader.get_tile(z16(1, 1)).await.unwrap().unwrap();
    assert_eq!(&tile[..4], b"RIFF");
    assert_eq!(&tile[8..12], b"WEBP");
}

#[cfg(feature = "png")]
#[rstest]
#[case::rgba_deflate("rgba_deflate.tif", 4)]
#[case::rgb_zstd("rgb_zstd.tif", 3)]
#[case::gray_lzw("gray_lzw.tif", 1)]
#[tokio::test]
async fn lossless_tiles_are_reencoded_as_exact_png(#[case] name: &str, #[case] bands: u32) {
    let reader = open(name).await;
    assert_eq!(reader.info().format, TileFormat::Png);
    for (col, row) in [(0, 0), (1, 1)] {
        let tile = reader.get_tile(z16(col, row)).await.unwrap().unwrap();
        let (w, h, channels, pixels) = decode_png(&tile);
        assert_eq!(w, 256);
        assert_eq!(h, 256);
        assert_eq!(channels, bands as usize);
        assert_eq!(
            pixels,
            expected_tile(col * 256, row * 256, bands, 0),
            "tile {col},{row}"
        );
    }
    let overview = reader
        .get_tile(TileCoord::new(15, Z15_X, Z15_Y).unwrap())
        .await
        .unwrap();
    assert_eq!(decode_png(&overview.unwrap()).0, 256);
}

#[cfg(feature = "png")]
#[tokio::test]
async fn sparse_tiles_are_none() {
    let reader = open("sparse.tif").await;
    assert!(reader.get_tile(z16(0, 0)).await.unwrap().is_some());
    assert!(reader.get_tile(z16(1, 1)).await.unwrap().is_none());
}

#[rstest]
#[case::float32(
    "float32.tif",
    "only 8-bit unsigned samples are supported, found [32] bits of [Float]"
)]
#[case::utm(
    "utm.tif",
    "its CRS is EPSG:32630, but only EPSG:3857 (Web Mercator) is supported"
)]
#[case::unaligned(
    "unaligned.tif",
    "image 0 is not aligned to the zoom 16 tile grid; image 1 is not aligned to the zoom 15 tile grid"
)]
#[case::odd_resolution(
    "odd_resolution.tif",
    "image 0 has 2.627522847302934 m pixels, which is not a WebMercatorQuad zoom level for 256 px tiles; image 1 has 5.255045694605868 m pixels, which is not a WebMercatorQuad zoom level for 256 px tiles"
)]
#[tokio::test]
async fn unsupported_files_are_rejected_with_a_reason(#[case] name: &str, #[case] reason: &str) {
    assert_eq!(
        open_err(name).await,
        format!(
            "{} cannot be read as tiles: {reason}",
            fixture(name).display()
        )
    );
}

#[tokio::test]
async fn malformed_files_are_errors_not_panics() {
    let garbage = MemoryBackend::new(vec![0x49, 0x49, 0x2A, 0, 8, 0, 0, 0, 0xFF, 0xFF], "garbage");
    AsyncCogReader::try_from_backend(garbage).await.unwrap_err();

    let empty = MemoryBackend::new(Vec::new(), "empty");
    AsyncCogReader::try_from_backend(empty).await.unwrap_err();

    let full = std::fs::read(fixture("rgb_jpeg.tif")).unwrap();
    let truncated = MemoryBackend::new(full[..600].to_vec(), "truncated");
    AsyncCogReader::try_from_backend(truncated)
        .await
        .unwrap_err();
}

#[tokio::test]
async fn opening_reads_headers_in_one_request() {
    let backend = Counting::new(memory("rgb_jpeg.tif"));
    let reads = Arc::clone(&backend.reads);
    let reader = AsyncCogReader::try_from_backend(backend).await.unwrap();
    assert_eq!(reads.load(Ordering::SeqCst), 1);
    reader.get_tile(z16(0, 0)).await.unwrap().unwrap();
    assert_eq!(
        reads.load(Ordering::SeqCst),
        2,
        "a passthrough tile is one range read"
    );
}

#[tokio::test]
async fn backend_errors_keep_their_type() {
    let full = std::fs::read(fixture("rgb_jpeg.tif")).unwrap();
    // Headers are intact, but tile data is cut off.
    let backend = MemoryBackend::new(full[..40_000].to_vec(), "cut");
    let reader = AsyncCogReader::try_from_backend(backend).await.unwrap();
    let error = reader.get_tile(z16(1, 1)).await.unwrap_err();
    assert!(matches!(error, CogError::InvalidRange { .. }), "{error:?}");
}

#[cfg(feature = "fs")]
#[tokio::test]
async fn local_files() {
    let backend = cog_tiles::FileBackend::open(fixture("rgb_jpeg.tif"))
        .await
        .unwrap();
    let reader = AsyncCogReader::try_from_backend(backend).await.unwrap();
    assert!(reader.get_tile(z16(0, 1)).await.unwrap().is_some());
    cog_tiles::FileBackend::open(fixture("missing.tif"))
        .await
        .unwrap_err();
}

const PHOTOMETRIC: u16 = 262;
const MODEL_PIXEL_SCALE: u16 = 33550;
const MODEL_TIEPOINT: u16 = 33922;
const PHOTOMETRIC_RGB: f64 = 2.0;
const PHOTOMETRIC_YCBCR: f64 = 6.0;

#[tokio::test]
async fn rgb_jpeg_tiles_pass_through_too() {
    // The same tiles, labelled RGB instead of YCbCr.
    let rgb = patched("rgb_jpeg.tif", |bytes| {
        patch_tag(bytes, PHOTOMETRIC, 0, |v| {
            if v == PHOTOMETRIC_YCBCR {
                PHOTOMETRIC_RGB
            } else {
                v
            }
        });
    });
    let rgb = AsyncCogReader::try_from_backend(rgb).await.unwrap();
    assert_eq!(rgb.info().format, TileFormat::Jpeg);
    let ycbcr = open("rgb_jpeg.tif").await;
    assert_eq!(
        rgb.get_tile(z16(1, 0)).await.unwrap(),
        ycbcr.get_tile(z16(1, 0)).await.unwrap()
    );
}

#[tokio::test]
async fn misaligned_overviews_are_left_out() {
    // Moving the image one z16 tile east keeps it on the z16 grid, but puts its z15 overview
    // half a tile off the z15 grid.
    let shifted = patched("rgb_jpeg.tif", |bytes| {
        patch_tag(bytes, MODEL_TIEPOINT, 3, |x| x + cog_tiles::tile_extent(16));
    });
    let reader = AsyncCogReader::try_from_backend(shifted).await.unwrap();
    assert_eq!(reader.info().min_zoom, 16);
    assert_eq!(reader.info().max_zoom, 16);
    assert_eq!(
        reader.skipped_images(),
        [SkippedImage::Misaligned { index: 1, zoom: 15 }]
    );
    let tile = TileCoord::new(16, Z15_X * 2 + 1, Z15_Y * 2).unwrap();
    assert!(reader.get_tile(tile).await.unwrap().is_some());
    assert!(
        reader
            .get_tile(TileCoord::new(15, Z15_X, Z15_Y).unwrap())
            .await
            .unwrap()
            .is_none()
    );
    let aligned = open("rgb_jpeg.tif").await;
    assert_eq!(aligned.skipped_images(), &[] as &[SkippedImage]);
}

#[tokio::test]
async fn pixels_within_a_centimeter_of_square_are_square() {
    let open_with_height_change = |change: f64| async move {
        let backend = patched("rgb_jpeg.tif", |bytes| {
            patch_tag(bytes, MODEL_PIXEL_SCALE, 1, |y| y + change);
        });
        AsyncCogReader::try_from_backend(backend).await
    };
    open_with_height_change(0.005).await.unwrap();
    let error = open_with_height_change(0.02).await.unwrap_err();
    assert_eq!(
        error.to_string(),
        format!(
            "{} cannot be read as tiles: pixels are not square (2.388657133911758 x 2.408657133911758 m)",
            fixture("rgb_jpeg.tif").display()
        )
    );
}
