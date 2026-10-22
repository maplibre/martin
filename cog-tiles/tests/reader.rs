#![allow(clippy::unwrap_used)]

mod common;

use std::sync::Arc;
use std::sync::atomic::Ordering;

use cog_tiles::{AsyncCogReader, CogError, MemoryBackend, SkippedImage, TileCoord, TileFormat};
use common::*;
use rstest::rstest;

async fn open(name: &str) -> AsyncCogReader<MemoryBackend> {
    AsyncCogReader::try_from_backend(memory(name)).await.unwrap()
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
#[case::utm("utm.tif", "its CRS is EPSG:32630, but only EPSG:3857 (Web Mercator) is supported")]
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
        format!("{} cannot be read as tiles: {reason}", fixture(name).display())
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
    AsyncCogReader::try_from_backend(truncated).await.unwrap_err();
}

#[tokio::test]
async fn opening_reads_headers_in_one_request() {
    let backend = Counting::new(memory("rgb_jpeg.tif"));
    let reads = Arc::clone(&backend.reads);
    let reader = AsyncCogReader::try_from_backend(backend).await.unwrap();
    assert_eq!(reads.load(Ordering::SeqCst), 1);
    reader.get_tile(z16(0, 0)).await.unwrap().unwrap();
    assert_eq!(reads.load(Ordering::SeqCst), 2, "a passthrough tile is one range read");
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
    let backend = cog_tiles::FileBackend::open(fixture("rgb_jpeg.tif")).await.unwrap();
    let reader = AsyncCogReader::try_from_backend(backend).await.unwrap();
    assert!(reader.get_tile(z16(0, 1)).await.unwrap().is_some());
    cog_tiles::FileBackend::open(fixture("missing.tif")).await.unwrap_err();
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
            if v == PHOTOMETRIC_YCBCR { PHOTOMETRIC_RGB } else { v }
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

#[tokio::test]
async fn striped_uncompressed_geotiffs_are_cut_into_png_tiles() {
    let tile = TileCoord::new(16, 10_484, 25_324).unwrap();
    let [min_x, _, _, max_y] = tile.mercator_bounds();
    let bytes = striped_tiff(&[(512, 80)], 0, Some([min_x, max_y, cog_tiles::resolution(17, 256)]));
    let backend = MemoryBackend::new(bytes, "striped.tif".to_owned());

    let reader = AsyncCogReader::try_from_backend(backend).await.unwrap();

    let info = reader.info();
    assert_eq!((info.min_zoom, info.max_zoom, info.tile_size), (17, 17, 256));
    assert_eq!(info.format, TileFormat::Png);
    let south_east = TileCoord::new(17, 20_969, 50_649).unwrap();
    let png = reader.get_tile(south_east).await.unwrap().unwrap();
    assert_eq!(decode_png(&png), (256, 256, 3, expected_tile(256, 256, 3, 0)));
}

#[tokio::test]
async fn opening_a_file_whose_ifd_follows_its_pixels_reads_only_the_header_and_the_ifd() {
    let tile = TileCoord::new(16, 10_484, 25_324).unwrap();
    let [min_x, _, _, max_y] = tile.mercator_bounds();
    let bytes = striped_tiff(&[(1024, 80)], 0, Some([min_x, max_y, cog_tiles::resolution(18, 256)]));
    let backend = Counting::new(MemoryBackend::new(bytes, "striped.tif".to_owned()));
    let reads = Arc::clone(&backend.reads);
    let bytes_read = Arc::clone(&backend.bytes);

    AsyncCogReader::try_from_backend(backend).await.unwrap();

    assert_eq!(reads.load(Ordering::SeqCst), 2);
    let bytes_read = bytes_read.load(Ordering::SeqCst);
    assert!(bytes_read <= 2 * 32 * 1024, "read {bytes_read} of 3,146,194 bytes");
}

#[tokio::test]
async fn pages_that_halve_in_size_are_overviews() {
    let tile = TileCoord::new(16, 10_484, 25_324).unwrap();
    let [min_x, _, _, max_y] = tile.mercator_bounds();
    let bytes = striped_tiff(
        &[(512, 80), (256, 256)],
        2,
        Some([min_x, max_y, cog_tiles::resolution(17, 256)]),
    );
    let backend = MemoryBackend::new(bytes, "pages.tif".to_owned());

    let reader = AsyncCogReader::try_from_backend(backend).await.unwrap();

    let info = reader.info();
    assert_eq!((info.min_zoom, info.max_zoom), (16, 17));
    let png = reader.get_tile(tile).await.unwrap().unwrap();
    assert_eq!(decode_png(&png), (256, 256, 3, expected_tile(0, 0, 3, 1)));
}

#[tokio::test]
async fn a_page_that_does_not_halve_in_size_is_skipped_and_the_next_halving_page_is_an_overview() {
    let tile = TileCoord::new(16, 10_484, 25_324).unwrap();
    let [min_x, _, _, max_y] = tile.mercator_bounds();
    let bytes = striped_tiff(
        &[(512, 80), (384, 128), (256, 256)],
        2,
        Some([min_x, max_y, cog_tiles::resolution(17, 256)]),
    );
    let backend = MemoryBackend::new(bytes, "pages.tif".to_owned());

    let reader = AsyncCogReader::try_from_backend(backend).await.unwrap();

    assert_eq!((reader.info().min_zoom, reader.info().max_zoom), (16, 17));
    let skipped: Vec<String> = reader.skipped_images().iter().map(ToString::to_string).collect();
    assert_eq!(
        skipped,
        ["image 1 is a 384x384 px page, but an overview page must be 256x256 px (half the previous image)"]
    );
    let png = reader.get_tile(tile).await.unwrap().unwrap();
    assert_eq!(decode_png(&png), (256, 256, 3, expected_tile(0, 0, 3, 2)));
}

#[tokio::test]
async fn an_overview_page_stored_differently_names_the_tag_and_both_values() {
    let tile = TileCoord::new(16, 10_484, 25_324).unwrap();
    let [min_x, _, _, max_y] = tile.mercator_bounds();
    let mut bytes = striped_tiff(
        &[(512, 80), (256, 256)],
        2,
        Some([min_x, max_y, cog_tiles::resolution(17, 256)]),
    );
    patch_page_tag(&mut bytes, 1, PHOTOMETRIC, 0, |_| PHOTOMETRIC_YCBCR);
    let backend = MemoryBackend::new(bytes, "pages.tif".to_owned());

    let error = AsyncCogReader::try_from_backend(backend).await.unwrap_err();

    assert_eq!(
        error.to_string(),
        "pages.tif cannot be read as tiles: image 1 has PhotometricInterpretation YCbCr, but the full-resolution image has RGB"
    );
}

#[tokio::test]
async fn tiles_in_one_row_of_a_striped_image_share_one_read() {
    let tile = TileCoord::new(16, 10_484, 25_324).unwrap();
    let [min_x, _, _, max_y] = tile.mercator_bounds();
    let bytes = striped_tiff(&[(1024, 80)], 0, Some([min_x, max_y, cog_tiles::resolution(18, 256)]));
    let backend = Counting::new(MemoryBackend::new(bytes, "striped.tif".to_owned()));
    let reads = Arc::clone(&backend.reads);
    let reader = AsyncCogReader::try_from_backend(backend).await.unwrap();
    let opened = reads.load(Ordering::SeqCst);

    let row = [41_936, 41_937, 41_938, 41_939].map(|x| TileCoord::new(18, x, 101_297).unwrap());
    let tiles = futures::future::join_all(row.map(|coord| reader.get_tile_pixels(coord))).await;

    assert_eq!(reads.load(Ordering::SeqCst) - opened, 1);
    for (col, tile) in (0..).zip(tiles) {
        assert_eq!(tile.unwrap().unwrap().data(), expected_tile(col * 256, 256, 3, 0));
    }
    reader.get_tile_pixels(row[0]).await.unwrap().unwrap();
    assert_eq!(reads.load(Ordering::SeqCst) - opened, 1);
}

#[rstest]
#[case::compression(
    259,
    0,
    5.0,
    "image 0 uses LZW compressed strips, only uncompressed strips are supported"
)]
#[case::planar_configuration(284, 0, 2.0, "bands are stored separately (PlanarConfiguration=2)")]
#[case::rows_per_strip(
    278,
    0,
    100.0,
    "image 0 needs 6 strips of 100 rows, but has 7 StripOffsets and 7 StripByteCounts"
)]
#[case::strip_byte_counts(279, 2, 122_879.0, "strip 2 of image 0 has 122879 bytes, expected 122880")]
#[case::orientation(274, 0, 3.0, "image 0 has Orientation 3, only 1 (top-left) is supported")]
#[case::fill_order(266, 0, 2.0, "image 0 has FillOrder 2, only 1 is supported")]
#[tokio::test]
async fn a_striped_file_that_cannot_be_read_names_the_tag_that_prevents_it(
    #[case] tag: u16,
    #[case] index: usize,
    #[case] value: f64,
    #[case] reason: &str,
) {
    let tile = TileCoord::new(16, 10_484, 25_324).unwrap();
    let [min_x, _, _, max_y] = tile.mercator_bounds();
    let mut bytes = striped_tiff(&[(512, 80)], 0, Some([min_x, max_y, cog_tiles::resolution(17, 256)]));
    patch_tag(&mut bytes, tag, index, |_| value);
    let backend = MemoryBackend::new(bytes, "striped.tif".to_owned());

    let error = AsyncCogReader::try_from_backend(backend).await.unwrap_err();

    assert_eq!(
        error.to_string(),
        format!("striped.tif cannot be read as tiles: {reason}")
    );
}
