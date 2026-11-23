#![allow(clippy::unwrap_used)]

mod common;

use cog_tiles::{AsyncCogReader, HALF_EARTH, MemoryBackend, PixelWindow, TileCoord, resolution};
use common::*;
use rstest::rstest;

async fn open(name: &str) -> AsyncCogReader<MemoryBackend> {
    AsyncCogReader::try_from_backend(memory(name)).await.unwrap()
}

fn z16(col: u32, row: u32) -> TileCoord {
    TileCoord::new(16, Z15_X * 2 + col, Z15_Y * 2 + row).unwrap()
}

#[rstest]
#[case::rgba_deflate("rgba_deflate.tif", 4, 0.0)]
#[case::rgb_zstd("rgb_zstd.tif", 3, 0.0)]
#[case::gray_lzw("gray_lzw.tif", 1, 0.0)]
#[case::rgb_jpeg("rgb_jpeg.tif", 3, 4.0)]
#[cfg_attr(feature = "webp", case::rgb_webp("rgb_webp.tif", 3, 4.0))]
#[tokio::test]
async fn tiles_decode_to_the_pixels_of_the_file(#[case] name: &str, #[case] bands: u32, #[case] max_diff: f64) {
    let reader = open(name).await;
    for (col, row) in [(0, 0), (1, 1)] {
        let pixels = reader.get_tile_pixels(z16(col, row)).await.unwrap().unwrap();
        assert_eq!(pixels.size(), 256);
        assert_eq!(pixels.bands(), bands as usize);
        let diff = mean_abs_diff(pixels.data(), &expected_tile(col * 256, row * 256, bands, 0));
        assert!(diff <= max_diff, "tile {col},{row} differs by {diff}");
    }
}

#[tokio::test]
async fn sparse_and_missing_tiles_decode_to_none() {
    let reader = open("sparse.tif").await;
    assert!(reader.get_tile_pixels(z16(0, 0)).await.unwrap().is_some());
    assert_eq!(reader.get_tile_pixels(z16(1, 1)).await.unwrap(), None);
    assert_eq!(reader.get_tile_pixels(z16(2, 0)).await.unwrap(), None);
}

#[tokio::test]
async fn a_window_stitches_the_pixels_of_the_tiles_it_touches() {
    let reader = open("rgb_zstd.tif").await;
    let (left, top, right, bottom) = (100_u32, 50_u32, 400_u32, 300_u32);
    let res = resolution(16, 256);
    let origin_x = f64::from(Z15_X * 2 * 256).mul_add(res, -HALF_EARTH);
    let origin_y = f64::from(Z15_Y * 2 * 256).mul_add(-res, HALF_EARTH);
    let window = PixelWindow::from_mercator_bounds(
        16,
        256,
        [
            (f64::from(left) + 0.25).mul_add(res, origin_x),
            (f64::from(bottom) - 0.25).mul_add(-res, origin_y),
            (f64::from(right) - 0.25).mul_add(res, origin_x),
            (f64::from(top) + 0.25).mul_add(-res, origin_y),
        ],
    )
    .unwrap();
    assert_eq!((window.width(), window.height()), (right - left, bottom - top));
    let tiles: Vec<_> = window.tiles().collect();
    assert_eq!(tiles, [z16(0, 0), z16(1, 0), z16(0, 1), z16(1, 1)]);

    let mut raster = vec![0; (right - left) as usize * (bottom - top) as usize * 3];
    for coord in tiles {
        let pixels = reader.get_tile_pixels(coord).await.unwrap().unwrap();
        window.paste(&mut raster, coord, &pixels);
    }
    let mut expected = Vec::with_capacity(raster.len());
    for y in top..bottom {
        for x in left..right {
            for band in 0..3 {
                expected.push(pattern(x, y, band, 0));
            }
        }
    }
    assert_eq!(raster, expected);
    let [min_x, _, _, max_y] = window.mercator_bounds();
    assert!((min_x - f64::from(left).mul_add(res, origin_x)).abs() < 1e-6);
    assert!((max_y - f64::from(top).mul_add(-res, origin_y)).abs() < 1e-6);
}
