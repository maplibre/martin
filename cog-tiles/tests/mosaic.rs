#![cfg(feature = "mosaic")]
#![allow(clippy::unwrap_used, clippy::panic)]

mod common;

use std::path::{Path as StdPath, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use cog_tiles::{
    AsyncCogMosaic, AsyncCogReader, CogError, FileBackend, MosaicFileError, MosaicInfo, MosaicOptions,
    MosaicTemplateError, TileCoord, TileFormat,
};
use common::{expected_tile, striped_tiff};
use object_store::memory::InMemory;
use object_store::path::Path;
use object_store::{ObjectStoreExt as _, PutPayload};
use rstest::rstest;

fn mosaic_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/cog/mosaic")
}

async fn single_tile(name: &str, coord: TileCoord) -> Option<bytes::Bytes> {
    let backend = FileBackend::open(mosaic_dir().join(name)).await.unwrap();
    let reader = AsyncCogReader::try_from_backend(backend).await.unwrap();
    reader.get_tile(coord).await.unwrap()
}

#[rstest]
#[case("plain.tif", MosaicTemplateError::MissingCoordinates)]
#[case("{z}/{x}.tif", MosaicTemplateError::MissingCoordinates)]
#[case("{z}/{-y}.tif", MosaicTemplateError::MissingCoordinates)]
#[case("{q}.tif", MosaicTemplateError::UnknownPlaceholder("q".to_owned()))]
#[case("{quadkey.tif", MosaicTemplateError::UnmatchedOpen)]
#[case("a}{quadkey}", MosaicTemplateError::UnmatchedClose)]
#[case("../{quadkey}.tif", MosaicTemplateError::NotRelative)]
#[case("{quadkey}/../x.tif", MosaicTemplateError::NotRelative)]
#[case("/data/{quadkey}.tif", MosaicTemplateError::NotRelative)]
#[tokio::test]
async fn invalid_templates_are_rejected(#[case] template: &str, #[case] expected: MosaicTemplateError) {
    let error = AsyncCogMosaic::open_dir(mosaic_dir(), template, MosaicOptions::new(16))
        .await
        .unwrap_err();
    let CogError::InvalidTemplate { reason, .. } = error else {
        panic!("{error:?}");
    };
    assert_eq!(reason, expected);
}

#[tokio::test]
async fn a_file_zoom_above_the_highest_zoom_is_rejected() {
    let error = AsyncCogMosaic::open_dir(mosaic_dir(), "{quadkey}.tif", MosaicOptions::new(31))
        .await
        .unwrap_err();
    assert!(
        matches!(error, CogError::FileZoomTooHigh { file_zoom: 31 }),
        "{error:?}"
    );
}

#[rstest]
#[case("{quadkey}.tif", "0313131311301120.tif", "0313131311301120.tif", Some((32_748, 21_794)))]
#[case("sub/{quadkey}.tif", "sub/0313131311301123.tif", "0313131311301123.tif", Some((32_749, 21_795)))]
#[case("{z}/{x}/{y}.tif", "16/32748/21794.tif", "0313131311301120.tif", Some((32_748, 21_794)))]
#[case("{x}_{y}.tif", "32748_21794.tif", "0313131311301120.tif", Some((32_748, 21_794)))]
#[case("L16-{x}E-{-y}N.tif", "L16-32748E-43741N.tif", "0313131311301120.tif", Some((32_748, 21_794)))]
#[case("{z}/{x}/{-y}.tif", "16/32748/43741.tif", "0313131311301120.tif", Some((32_748, 21_794)))]
#[case("{x}_{y}.tif", "032748_21794.tif", "0313131311301120.tif", None)]
#[case("{z}/{x}/{-y}.tif", "16/32748/65536.tif", "0313131311301120.tif", None)]
#[case("{z}/{x}/{y}.tif", "15/32748/21794.tif", "0313131311301120.tif", None)]
#[case("{quadkey}.tif", "031313131130112.tif", "0313131311301120.tif", None)]
#[case("{quadkey}.tif", "0313131311301124.tif", "0313131311301120.tif", None)]
#[case("{quadkey}.tif", "0313131311301120.tif.aux.xml", "0313131311301120.tif", None)]
#[case("{quadkey}.tif", "readme.txt", "0313131311301120.tif", None)]
#[case("sub/{quadkey}.tif", "0313131311301120.tif", "0313131311301120.tif", None)]
#[tokio::test]
async fn file_names_are_matched_to_their_tiles(
    #[case] template: &str,
    #[case] name: &str,
    #[case] fixture: &str,
    #[case] served: Option<(u32, u32)>,
) {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join(name);
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::copy(mosaic_dir().join(fixture), &file).unwrap();
    let result = AsyncCogMosaic::open_dir(dir.path(), template, MosaicOptions::new(16)).await;

    let Some((x, y)) = served else {
        let error = result.unwrap_err();
        assert!(matches!(error, CogError::MosaicEmpty { .. }), "{error:?}");
        return;
    };
    let mosaic = result.unwrap();
    let coord = TileCoord::new(16, x, y).unwrap();
    let tile = mosaic.get_tile(coord).await.unwrap();
    assert!(tile.is_some());
    assert_eq!(tile, single_tile(fixture, coord).await);
}

#[tokio::test]
async fn a_directory_mosaic_serves_each_tile_from_its_file() {
    let mosaic = AsyncCogMosaic::open_dir(mosaic_dir(), "{quadkey}.tif", MosaicOptions::new(16))
        .await
        .unwrap();
    assert_eq!(
        *mosaic.info(),
        MosaicInfo {
            file_zoom: 16,
            max_zoom: 17,
            tile_size: 256,
            format: TileFormat::Png,
        }
    );

    for (name, x, y) in [
        ("0313131311301120.tif", 32_748, 21_794),
        ("0313131311301121.tif", 32_749, 21_794),
        ("0313131311301122.tif", 32_748, 21_795),
        ("0313131311301123.tif", 32_749, 21_795),
    ] {
        for coord in [
            TileCoord::new(16, x, y).unwrap(),
            TileCoord::new(17, x * 2 + 1, y * 2).unwrap(),
        ] {
            let tile = mosaic.get_tile(coord).await.unwrap();
            assert!(tile.is_some(), "{coord}");
            assert_eq!(tile, single_tile(name, coord).await, "{coord}");
        }
    }
}

#[rstest]
#[case::above_the_file_zoom(TileCoord::new(15, 16_374, 10_897).unwrap())]
#[case::no_file_for_this_tile(TileCoord::new(16, 32_750, 21_794).unwrap())]
#[case::below_a_missing_file(TileCoord::new(17, 65_500, 43_588).unwrap())]
#[case::deeper_than_the_files_go(TileCoord::new(18, 130_992, 87_176).unwrap())]
#[tokio::test]
async fn tiles_without_a_file_are_none(#[case] coord: TileCoord) {
    let mosaic = AsyncCogMosaic::open_dir(mosaic_dir(), "{quadkey}.tif", MosaicOptions::new(16))
        .await
        .unwrap();
    assert_eq!(mosaic.get_tile(coord).await.unwrap(), None);
}

#[tokio::test]
async fn a_missing_file_stays_missing_until_it_expires() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::copy(
        mosaic_dir().join("0313131311301120.tif"),
        dir.path().join("0313131311301120.tif"),
    )
    .unwrap();
    let mosaic = AsyncCogMosaic::open_dir(
        dir.path(),
        "{quadkey}.tif",
        MosaicOptions::new(16).with_file_expiry(Duration::from_mins(1)),
    )
    .await
    .unwrap();
    let ne = TileCoord::new(16, 32_749, 21_794).unwrap();
    assert_eq!(mosaic.get_tile(ne).await.unwrap(), None);

    std::fs::copy(
        mosaic_dir().join("0313131311301121.tif"),
        dir.path().join("0313131311301121.tif"),
    )
    .unwrap();
    assert_eq!(mosaic.get_tile(ne).await.unwrap(), None);
}

#[cfg(unix)]
#[tokio::test]
async fn an_opened_file_keeps_serving_after_it_is_removed() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::copy(
        mosaic_dir().join("0313131311301120.tif"),
        dir.path().join("0313131311301120.tif"),
    )
    .unwrap();
    let mosaic = AsyncCogMosaic::open_dir(dir.path(), "{quadkey}.tif", MosaicOptions::new(16))
        .await
        .unwrap();
    std::fs::remove_file(dir.path().join("0313131311301120.tif")).unwrap();

    let z17 = TileCoord::new(17, 65_497, 43_589).unwrap();
    assert!(mosaic.get_tile(z17).await.unwrap().is_some());
}

#[tokio::test]
async fn files_that_do_not_cover_their_tile_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::copy(
        mosaic_dir().join("0313131311301120.tif"),
        dir.path().join("0313131311301120.tif"),
    )
    .unwrap();
    let mosaic = AsyncCogMosaic::open_dir(dir.path(), "{quadkey}.tif", MosaicOptions::new(16))
        .await
        .unwrap();
    std::fs::copy(
        mosaic_dir().join("0313131311301120.tif"),
        dir.path().join("0313131311301121.tif"),
    )
    .unwrap();

    let ne = TileCoord::new(16, 32_749, 21_794).unwrap();
    let error = mosaic.get_tile(ne).await.unwrap_err();
    let CogError::MosaicFile { key, source } = error else {
        panic!("{error:?}");
    };
    assert_eq!(key, "0313131311301121.tif");
    let MosaicFileError::DoesNotCover { tile } = *source else {
        panic!("{source:?}");
    };
    assert_eq!(tile, ne);
}

#[tokio::test]
async fn files_with_a_different_format_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::copy(
        mosaic_dir().join("0313131311301120.tif"),
        dir.path().join("0313131311301120.tif"),
    )
    .unwrap();
    let mosaic = AsyncCogMosaic::open_dir(dir.path(), "{quadkey}.tif", MosaicOptions::new(16))
        .await
        .unwrap();
    // A JPEG file covering the whole z15 tile, named as the south-east z16 one.
    std::fs::copy(
        mosaic_dir().join("../synthetic/rgb_jpeg.tif"),
        dir.path().join("0313131311301123.tif"),
    )
    .unwrap();

    let se = TileCoord::new(16, 32_749, 21_795).unwrap();
    let error = mosaic.get_tile(se).await.unwrap_err();
    let CogError::MosaicFile { key, source } = error else {
        panic!("{error:?}");
    };
    assert_eq!(key, "0313131311301123.tif");
    assert!(
        matches!(
            *source,
            MosaicFileError::Layout {
                format: TileFormat::Jpeg,
                expected_format: TileFormat::Png,
                ..
            }
        ),
        "{source:?}"
    );
}

#[tokio::test]
async fn a_mosaic_without_matching_files_is_an_error() {
    let error = AsyncCogMosaic::open_dir(mosaic_dir(), "{quadkey}_missing.tif", MosaicOptions::new(16))
        .await
        .unwrap_err();
    assert!(matches!(error, CogError::MosaicEmpty { .. }), "{error:?}");
}

#[tokio::test]
async fn a_missing_mosaic_directory_is_an_error() {
    let error = AsyncCogMosaic::open_dir(
        StdPath::new(&mosaic_dir()).join("missing"),
        "{quadkey}.tif",
        MosaicOptions::new(16),
    )
    .await
    .unwrap_err();
    assert!(matches!(error, CogError::MosaicListing { .. }), "{error:?}");
}

#[tokio::test]
async fn an_object_store_mosaic_matches_a_directory_mosaic() {
    let store = Arc::new(InMemory::new());
    // Unrelated objects are skipped when looking for a file to probe.
    store
        .put(&Path::from("imagery/readme.txt"), PutPayload::from("hi"))
        .await
        .unwrap();
    for entry in std::fs::read_dir(mosaic_dir()).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy();
        let bytes = std::fs::read(&path).unwrap();
        let key = Path::from(format!("imagery/{name}"));
        store.put(&key, PutPayload::from(bytes)).await.unwrap();
    }
    let remote = AsyncCogMosaic::open_object_store(
        store,
        Path::from("imagery"),
        "{quadkey}.tif",
        "memory://imagery/{quadkey}.tif",
        MosaicOptions::new(16),
    )
    .await
    .unwrap();
    let local = AsyncCogMosaic::open_dir(mosaic_dir(), "{quadkey}.tif", MosaicOptions::new(16))
        .await
        .unwrap();

    assert_eq!(remote.info(), local.info());
    let coord = TileCoord::new(17, 65_499, 43_589).unwrap();
    let tile = remote.get_tile(coord).await.unwrap();
    assert!(tile.is_some());
    assert_eq!(tile, local.get_tile(coord).await.unwrap());
    let missing = TileCoord::new(16, 32_750, 21_794).unwrap();
    assert_eq!(remote.get_tile(missing).await.unwrap(), None);
}

#[tokio::test]
async fn files_without_geotiff_tags_are_placed_by_their_name() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("0230102033312300_rgb.tif"),
        striped_tiff(&[(512, 80), (256, 256)], 2, None),
    )
    .unwrap();
    std::fs::write(
        dir.path().join("0230102033312301_rgb.tif"),
        striped_tiff(&[(512, 80), (256, 256)], 2, None),
    )
    .unwrap();

    let mosaic = AsyncCogMosaic::open_dir(dir.path(), "{quadkey}_rgb.tif", MosaicOptions::new(16))
        .await
        .unwrap();

    assert_eq!(
        mosaic.info(),
        &MosaicInfo {
            file_zoom: 16,
            max_zoom: 17,
            tile_size: 256,
            format: TileFormat::Png,
        }
    );
    let z16_east = TileCoord::new(16, 10_485, 25_324).unwrap();
    let pixels = mosaic.get_tile_pixels(z16_east).await.unwrap().unwrap();
    assert_eq!(pixels.data(), expected_tile(0, 0, 3, 1));
    let z17_east_north_east = TileCoord::new(17, 20_971, 50_648).unwrap();
    let pixels = mosaic.get_tile_pixels(z17_east_north_east).await.unwrap().unwrap();
    assert_eq!(pixels.data(), expected_tile(256, 0, 3, 0));
}
