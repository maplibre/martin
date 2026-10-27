#![cfg(feature = "mosaic")]
#![allow(clippy::unwrap_used, clippy::panic, clippy::cast_precision_loss)]

mod common;

use std::path::PathBuf;
use std::sync::Arc;

use cog_tiles::{
    AsyncCogReader, AsyncIndexedMosaic, CogError, CogResult, FileBackend, IndexedMosaicInfo, IndexedMosaicOptions,
    MosaicFile, MosaicIndex, PixelWindow, TileCoord, TileFormat,
};
use common::decode_png;
use object_store::memory::InMemory;
use object_store::path::Path;
use object_store::{ObjectStore, ObjectStoreExt as _, PutPayload};
use rstest::rstest;

const UTM_BOUNDS: [f64; 4] = [-122.454_75, 37.790_5, -122.437_3, 37.802_2];

fn warped_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/cog/warped")
}

#[derive(Debug)]
struct Files(Vec<MosaicFile>);

#[async_trait::async_trait]
impl MosaicIndex for Files {
    async fn files_for(&self, _coord: TileCoord) -> CogResult<Arc<[MosaicFile]>> {
        Ok(self.0.clone().into())
    }
}

async fn store_with(files: &[(&str, Vec<u8>)]) -> Arc<dyn ObjectStore> {
    let store = Arc::new(InMemory::new());
    for (key, bytes) in files {
        store
            .put(&Path::from(*key), PutPayload::from(bytes.clone()))
            .await
            .unwrap();
    }
    store
}

fn file(store: &Arc<dyn ObjectStore>, key: &str, bounds: [f64; 4]) -> MosaicFile {
    MosaicFile {
        cache_key: format!("memory://{key}").into(),
        location: format!("memory://{key}").into(),
        store: Arc::clone(store),
        path: Path::from(key),
        bounds,
    }
}

async fn utm_mosaic(options: IndexedMosaicOptions) -> AsyncIndexedMosaic {
    let store = store_with(&[("utm.tif", std::fs::read(warped_dir().join("utm.tif")).unwrap())]).await;
    let utm = file(&store, "utm.tif", UTM_BOUNDS);
    AsyncIndexedMosaic::open(Arc::new(Files(vec![utm.clone()])), &utm, options)
        .await
        .unwrap()
}

fn mismatched_pixels(actual: &[u8], expected: &[u8]) -> usize {
    actual
        .as_chunks::<4>()
        .0
        .iter()
        .zip(expected.as_chunks::<4>().0)
        .filter(|(a, e)| a[3] != e[3] || (a[3] != 0 && a[..3] != e[..3]))
        .count()
}

#[rstest]
#[case::z13_from_the_coarsest_overview(13, 1309, 3165)]
#[case::z14_from_the_first_overview(14, 2619, 6331)]
#[case::z15_from_full_resolution(15, 5238, 12663)]
#[case::z16_inside(16, 10477, 25326)]
#[case::z17_inside(17, 20954, 50652)]
#[case::z17_across_the_east_edge(17, 20957, 50652)]
#[case::z17_across_the_nodata_edge(17, 20952, 50650)]
#[tokio::test]
async fn utm_tiles_match_gdalwarp(#[case] z: u8, #[case] x: u32, #[case] y: u32) {
    let mosaic = utm_mosaic(IndexedMosaicOptions::new(13, vec![0, 1, 2]).with_max_zoom(Some(17))).await;
    let expected = std::fs::read(warped_dir().join(format!("expected/{z}_{x}_{y}.png"))).unwrap();
    let (_, _, channels, expected) = decode_png(&expected);
    assert_eq!(channels, 4);

    let tile = mosaic
        .get_tile(TileCoord::new(z, x, y).unwrap())
        .await
        .unwrap()
        .unwrap();
    let (width, height, channels, actual) = decode_png(&tile);

    assert_eq!((width, height, channels), (256, 256, 4));
    let mismatched = mismatched_pixels(&actual, &expected);
    assert!(mismatched <= 8, "{mismatched} of 65536 pixels differ from gdalwarp");
}

#[tokio::test]
async fn a_tile_where_the_file_has_only_nodata_is_empty() {
    let mosaic = utm_mosaic(IndexedMosaicOptions::new(13, vec![0, 1, 2]).with_max_zoom(Some(17))).await;

    let tile = mosaic
        .get_tile(TileCoord::new(17, 20951, 50649).unwrap())
        .await
        .unwrap();

    assert_eq!(tile, None);
}

#[tokio::test]
async fn a_nodata_override_paints_what_the_file_marks_as_nodata() {
    let mosaic = utm_mosaic(
        IndexedMosaicOptions::new(13, vec![0, 1, 2])
            .with_max_zoom(Some(17))
            .with_nodata(Some(255)),
    )
    .await;

    let pixels = mosaic
        .get_tile_pixels(TileCoord::new(17, 20951, 50649).unwrap())
        .await
        .unwrap()
        .unwrap();

    assert!(pixels.data().as_chunks::<4>().0.contains(&[0, 0, 0, 255]));
}

#[rstest]
#[case::outside_the_file(TileCoord::new(17, 20960, 50652).unwrap())]
#[case::below_the_min_zoom(TileCoord::new(12, 654, 1582).unwrap())]
#[case::above_the_max_zoom(TileCoord::new(18, 41_908, 101_304).unwrap())]
#[tokio::test]
async fn tiles_without_data_are_empty(#[case] coord: TileCoord) {
    let mosaic = utm_mosaic(IndexedMosaicOptions::new(13, vec![0, 1, 2]).with_max_zoom(Some(17))).await;

    assert_eq!(mosaic.get_tile(coord).await.unwrap(), None);
}

#[tokio::test]
async fn the_max_zoom_defaults_to_where_the_probed_file_is_fully_detailed() {
    let mosaic = utm_mosaic(IndexedMosaicOptions::new(13, vec![0, 1, 2])).await;

    assert_eq!(
        mosaic.info(),
        &IndexedMosaicInfo {
            min_zoom: 13,
            max_zoom: 16,
            tile_size: 256,
            format: TileFormat::Png,
        }
    );
}

#[tokio::test]
async fn a_single_band_is_served_as_gray_with_alpha() {
    let mosaic = utm_mosaic(IndexedMosaicOptions::new(13, vec![3]).with_max_zoom(Some(17))).await;

    let tile = mosaic
        .get_tile(TileCoord::new(16, 10477, 25326).unwrap())
        .await
        .unwrap()
        .unwrap();

    assert_eq!(decode_png(&tile).2, 2);
}

#[rstest]
#[case::nodata("utm.tif")]
#[case::alpha("utm_alpha.tif")]
#[tokio::test]
async fn earlier_files_win_and_later_files_fill_their_gaps(#[case] first: &str) {
    let store = store_with(&[
        (first, std::fs::read(warped_dir().join(first)).unwrap()),
        (
            "utm_flat.tif",
            std::fs::read(warped_dir().join("utm_flat.tif")).unwrap(),
        ),
    ])
    .await;
    let first = file(&store, first, UTM_BOUNDS);
    let index = Arc::new(Files(vec![first.clone(), file(&store, "utm_flat.tif", UTM_BOUNDS)]));
    let mosaic = AsyncIndexedMosaic::open(
        index,
        &first,
        IndexedMosaicOptions::new(13, vec![0, 1, 2]).with_max_zoom(Some(17)),
    )
    .await
    .unwrap();
    let expected = std::fs::read(warped_dir().join("expected/two_files_17_20952_50650.png")).unwrap();
    let (_, _, _, expected) = decode_png(&expected);

    let tile = mosaic
        .get_tile(TileCoord::new(17, 20952, 50650).unwrap())
        .await
        .unwrap()
        .unwrap();

    let mismatched = mismatched_pixels(&decode_png(&tile).3, &expected);
    assert!(mismatched <= 8, "{mismatched} of 65536 pixels differ from gdalwarp");
}

#[tokio::test]
async fn files_shown_under_one_location_are_opened_by_their_cache_keys() {
    let store = store_with(&[
        ("utm.tif", std::fs::read(warped_dir().join("utm.tif")).unwrap()),
        (
            "utm_flat.tif",
            std::fs::read(warped_dir().join("utm_flat.tif")).unwrap(),
        ),
    ])
    .await;
    let first = MosaicFile {
        cache_key: "https://example.com/image?id=utm".into(),
        location: "https://example.com/image".into(),
        store: Arc::clone(&store),
        path: Path::from("utm.tif"),
        bounds: UTM_BOUNDS,
    };
    let second = MosaicFile {
        cache_key: "https://example.com/image?id=utm_flat".into(),
        location: "https://example.com/image".into(),
        store: Arc::clone(&store),
        path: Path::from("utm_flat.tif"),
        bounds: UTM_BOUNDS,
    };
    let mosaic = AsyncIndexedMosaic::open(
        Arc::new(Files(vec![first.clone(), second])),
        &first,
        IndexedMosaicOptions::new(13, vec![0, 1, 2]).with_max_zoom(Some(17)),
    )
    .await
    .unwrap();
    let expected = std::fs::read(warped_dir().join("expected/two_files_17_20952_50650.png")).unwrap();
    let (_, _, _, expected) = decode_png(&expected);

    let tile = mosaic
        .get_tile(TileCoord::new(17, 20952, 50650).unwrap())
        .await
        .unwrap()
        .unwrap();

    let mismatched = mismatched_pixels(&decode_png(&tile).3, &expected);
    assert!(mismatched <= 8, "{mismatched} of 65536 pixels differ from gdalwarp");
}

#[tokio::test]
async fn pixels_the_alpha_band_hides_are_not_painted() {
    let store = store_with(&[(
        "utm_alpha.tif",
        std::fs::read(warped_dir().join("utm_alpha.tif")).unwrap(),
    )])
    .await;
    let alpha = file(&store, "utm_alpha.tif", UTM_BOUNDS);
    let mosaic = AsyncIndexedMosaic::open(
        Arc::new(Files(vec![alpha.clone()])),
        &alpha,
        IndexedMosaicOptions::new(13, vec![0, 1, 2]).with_max_zoom(Some(17)),
    )
    .await
    .unwrap();
    let expected = std::fs::read(warped_dir().join("expected/17_20952_50650.png")).unwrap();
    let (_, _, _, expected) = decode_png(&expected);

    let tile = mosaic
        .get_tile(TileCoord::new(17, 20952, 50650).unwrap())
        .await
        .unwrap()
        .unwrap();

    let mismatched = mismatched_pixels(&decode_png(&tile).3, &expected);
    assert!(mismatched <= 8, "{mismatched} of 65536 pixels differ from gdalwarp");
}

#[tokio::test]
async fn a_tile_far_coarser_than_the_coarsest_overview_is_refused() {
    let store = store_with(&[(
        "utm_no_overviews.tif",
        std::fs::read(warped_dir().join("utm_no_overviews.tif")).unwrap(),
    )])
    .await;
    let bare = file(&store, "utm_no_overviews.tif", UTM_BOUNDS);
    let mosaic = AsyncIndexedMosaic::open(
        Arc::new(Files(vec![bare.clone()])),
        &bare,
        IndexedMosaicOptions::new(13, vec![0, 1, 2]),
    )
    .await
    .unwrap();

    let error = mosaic
        .get_tile(TileCoord::new(13, 1309, 3165).unwrap())
        .await
        .unwrap_err();

    assert_eq!(
        error.to_string(),
        "Unable to read mosaic file memory://utm_no_overviews.tif: InMemory/utm_no_overviews.tif cannot be read as tiles: zoom 13 tiles need the image downsampled 7.6 times, but at most 4 times is read; add overviews or raise the min zoom"
    );
}

#[rstest]
#[case::none(vec![])]
#[case::two(vec![0, 1])]
#[case::four(vec![0, 1, 2, 3])]
#[tokio::test]
async fn only_one_or_three_bands_can_be_served(#[case] bands: Vec<usize>) {
    let store = store_with(&[("utm.tif", std::fs::read(warped_dir().join("utm.tif")).unwrap())]).await;
    let utm = file(&store, "utm.tif", UTM_BOUNDS);

    let error = AsyncIndexedMosaic::open(
        Arc::new(Files(vec![utm.clone()])),
        &utm,
        IndexedMosaicOptions::new(13, bands.clone()),
    )
    .await
    .unwrap_err();

    assert!(
        matches!(error, CogError::MosaicBands { count } if count == bands.len()),
        "{error:?}"
    );
}

#[tokio::test]
async fn a_band_the_file_lacks_is_reported() {
    let store = store_with(&[("utm.tif", std::fs::read(warped_dir().join("utm.tif")).unwrap())]).await;
    let utm = file(&store, "utm.tif", UTM_BOUNDS);

    let error = AsyncIndexedMosaic::open(
        Arc::new(Files(vec![utm.clone()])),
        &utm,
        IndexedMosaicOptions::new(13, vec![0, 1, 4]),
    )
    .await
    .unwrap_err();

    assert_eq!(
        error.to_string(),
        "Unable to read mosaic file memory://utm.tif: InMemory/utm.tif cannot be read as tiles: band 5 is requested, but the file has 4 bands"
    );
}

#[tokio::test]
async fn a_file_in_an_unsupported_crs_is_reported() {
    let store = store_with(&[("polar.tif", std::fs::read(warped_dir().join("polar.tif")).unwrap())]).await;
    let polar = file(&store, "polar.tif", [-45.0, 89.0, 135.0, 90.0]);

    let error = AsyncIndexedMosaic::open(
        Arc::new(Files(vec![polar.clone()])),
        &polar,
        IndexedMosaicOptions::new(0, vec![0]),
    )
    .await
    .unwrap_err();

    assert_eq!(
        error.to_string(),
        "Unable to read mosaic file memory://polar.tif: InMemory/polar.tif cannot be read as tiles: its CRS is EPSG:3413, but only Web Mercator and UTM zones (EPSG:326xx, 327xx, 269xx, 258xx) can be reprojected"
    );
}

#[tokio::test]
async fn a_web_mercator_file_on_the_tile_grid_keeps_its_pixels() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/cog/usda_naip_256_lzw_rgb_z2.tif");
    let reader = AsyncCogReader::try_from_backend(FileBackend::open(&path).await.unwrap())
        .await
        .unwrap();
    let info = *reader.info();
    let store = store_with(&[("aligned.tif", std::fs::read(&path).unwrap())]).await;
    let aligned = file(&store, "aligned.tif", info.bounds);
    let mosaic = AsyncIndexedMosaic::open(
        Arc::new(Files(vec![aligned.clone()])),
        &aligned,
        IndexedMosaicOptions::new(info.min_zoom, vec![0, 1, 2]),
    )
    .await
    .unwrap();
    let coord = PixelWindow::from_lnglat_bounds(info.max_zoom, 256, info.bounds)
        .unwrap()
        .tiles()
        .next()
        .unwrap();

    let expected = reader.get_tile_pixels(coord).await.unwrap().unwrap();
    let actual = mosaic.get_tile_pixels(coord).await.unwrap().unwrap();

    let rgb: Vec<u8> = actual
        .data()
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|pixel| &pixel[..3])
        .copied()
        .collect();
    assert_eq!(rgb, expected.data());
    assert_eq!(mosaic.info().max_zoom, info.max_zoom);
}
