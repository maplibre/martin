//! `pack` and `unpack` in the `mbtiles` CLI.

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

use martin_e2e_tests::{GZIP_MAGIC, MbtilesCli, Tile, mbtiles_fixture, temp_dir, tiles};
use rstest::rstest;

fn tile_tree(dir: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut tree = BTreeMap::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(current) = pending.pop() {
        let entries = fs::read_dir(&current).expect("failed to read a tile tree directory");
        for entry in entries {
            let path = entry.expect("failed to read a tile tree entry").path();
            if path.is_dir() {
                pending.push(path);
            } else {
                let relative = path
                    .strip_prefix(dir)
                    .expect("a tile is below the tile tree root")
                    .to_path_buf();
                tree.insert(relative, fs::read(&path).expect("failed to read a tile"));
            }
        }
    }
    assert!(!tree.is_empty(), "the tile tree is empty");
    tree
}

fn flip_y(path: &Path) -> PathBuf {
    let mut parts = path.iter();
    let z = parts
        .next()
        .expect("a tile path starts with its zoom level");
    let x = parts.next().expect("a tile path has an x directory");
    let name = parts.next().expect("a tile path has a y file name");
    let zoom: u32 = z
        .to_str()
        .expect("a zoom level is utf-8")
        .parse()
        .expect("a zoom level is numeric");
    let (y, extension) = name
        .to_str()
        .expect("a y file name is utf-8")
        .split_once('.')
        .expect("a y file name has an extension");
    let y: u32 = y.parse().expect("a y file name is numeric");
    let flipped = (1 << zoom) - 1 - y;
    Path::new(z).join(x).join(format!("{flipped}.{extension}"))
}

#[tokio::test]
async fn unpack_names_tiles_after_the_stored_format() {
    let dir = temp_dir();
    let source = mbtiles_fixture(dir.path(), "world_cities").await;
    let tree_dir = dir.path().join("xyz");

    MbtilesCli::new("unpack")
        .arg(&source)
        .arg(&tree_dir)
        .run()
        .await;

    let tree = tile_tree(&tree_dir);
    assert!(
        tree.contains_key(Path::new("0/0/0.pbf")),
        "unpacked tiles: {:?}",
        tree.keys().collect::<Vec<_>>()
    );
    assert!(
        tree.keys()
            .all(|path| path.extension() == Some(OsStr::new("pbf"))),
        "unpacked tiles: {:?}",
        tree.keys().collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn pack_unpack_round_trips_a_vector_tile_tree() {
    let dir = temp_dir();
    let source = mbtiles_fixture(dir.path(), "world_cities").await;
    let tree_dir = dir.path().join("xyz");
    let packed = dir.path().join("repacked.mbtiles");
    let repacked_tree_dir = dir.path().join("xyz_again");

    MbtilesCli::new("unpack")
        .arg(&source)
        .arg(&tree_dir)
        .run()
        .await;
    MbtilesCli::new("pack")
        .arg(&tree_dir)
        .arg(&packed)
        .run()
        .await;
    MbtilesCli::new("unpack")
        .arg(&packed)
        .arg(&repacked_tree_dir)
        .run()
        .await;

    assert_eq!(tile_tree(&tree_dir), tile_tree(&repacked_tree_dir));
}

#[tokio::test]
async fn unpack_with_the_tms_scheme_flips_the_y_coordinate() {
    let dir = temp_dir();
    let source = mbtiles_fixture(dir.path(), "world_cities").await;
    let xyz_dir = dir.path().join("xyz");
    let tms_dir = dir.path().join("tms");

    MbtilesCli::new("unpack")
        .arg(&source)
        .arg(&xyz_dir)
        .run()
        .await;
    MbtilesCli::new("unpack")
        .arg(&source)
        .arg(&tms_dir)
        .arg("--scheme")
        .arg("tms")
        .run()
        .await;

    let xyz = tile_tree(&xyz_dir);
    let expected = xyz
        .iter()
        .map(|(path, data)| (flip_y(path), data.clone()))
        .collect::<BTreeMap<_, _>>();
    assert_ne!(xyz.keys().collect::<Vec<_>>(), expected.keys().collect::<Vec<_>>());
    assert_eq!(tile_tree(&tms_dir), expected);
}

#[tokio::test]
async fn packing_without_compression_round_trips_a_vector_tile_tree() {
    let dir = temp_dir();
    let source = mbtiles_fixture(dir.path(), "world_cities").await;
    let tree_dir = dir.path().join("xyz");
    let packed = dir.path().join("raw.mbtiles");
    let unpacked_dir = dir.path().join("raw_out");

    MbtilesCli::new("unpack")
        .arg(&source)
        .arg(&tree_dir)
        .run()
        .await;
    MbtilesCli::new("pack")
        .arg(&tree_dir)
        .arg(&packed)
        .arg("--compress")
        .arg("none")
        .run()
        .await;
    MbtilesCli::new("unpack")
        .arg(&packed)
        .arg(&unpacked_dir)
        .run()
        .await;

    assert_eq!(tile_tree(&tree_dir), tile_tree(&unpacked_dir));
}

#[rstest]
#[case::auto(&[], true)]
#[case::none(&["--compress", "none"], false)]
#[tokio::test]
async fn pack_gzips_vector_tiles_unless_compression_is_disabled(
    #[case] extra_args: &[&str],
    #[case] gzipped: bool,
) {
    let dir = temp_dir();
    let source = mbtiles_fixture(dir.path(), "world_cities").await;
    let tree_dir = dir.path().join("xyz");
    let packed = dir.path().join("packed.mbtiles");

    MbtilesCli::new("unpack")
        .arg(&source)
        .arg(&tree_dir)
        .run()
        .await;
    let mut command = MbtilesCli::new("pack").arg(&tree_dir).arg(&packed);
    for arg in extra_args {
        command = command.arg(*arg);
    }
    command.run().await;

    let packed_tiles = tiles(&packed).await;
    assert!(!packed_tiles.is_empty());
    for (z, x, y, data) in &packed_tiles {
        assert_eq!(
            data.starts_with(&GZIP_MAGIC),
            gzipped,
            "tile {z}/{x}/{y} is gzipped: {}",
            data.starts_with(&GZIP_MAGIC)
        );
    }
}

#[rstest]
#[case::xyz("xyz")]
#[case::tms("tms")]
#[tokio::test]
async fn uncompressed_image_tiles_round_trip_byte_for_byte(#[case] scheme: &str) {
    let dir = temp_dir();
    let source = mbtiles_fixture(dir.path(), "webp-no-primary").await;
    let tree_dir = dir.path().join(scheme);
    let packed = dir.path().join(format!("{scheme}.mbtiles"));

    MbtilesCli::new("unpack")
        .arg(&source)
        .arg(&tree_dir)
        .arg("--scheme")
        .arg(scheme)
        .run()
        .await;
    MbtilesCli::new("pack")
        .arg(&tree_dir)
        .arg(&packed)
        .arg("--scheme")
        .arg(scheme)
        .run()
        .await;

    let source_tiles = tiles(&source).await;
    assert!(!source_tiles.is_empty());
    assert_eq!(source_tiles, tiles(&packed).await);
}

#[tokio::test]
async fn unpack_fails_on_a_missing_file() {
    let dir = temp_dir();

    let output = MbtilesCli::new("unpack")
        .arg(dir.path().join("does-not-exist.mbtiles"))
        .arg(dir.path().join("out"))
        .run_failing()
        .await;

    assert!(output.contains("Input file does not exist"), "unexpected output:\n{output}");
}

#[tokio::test]
async fn unpack_fails_when_the_metadata_has_no_format() {
    let dir = temp_dir();
    let source = mbtiles_fixture(dir.path(), "geography-class-png").await;

    let output = MbtilesCli::new("unpack")
        .arg(&source)
        .arg(dir.path().join("out"))
        .run_failing()
        .await;

    assert!(
        output.contains("No format specified in MBTiles metadata"),
        "unexpected output:\n{output}"
    );
}

#[tokio::test]
async fn pack_fails_on_a_missing_directory() {
    let dir = temp_dir();

    let output = MbtilesCli::new("pack")
        .arg(dir.path().join("does-not-exist"))
        .arg(dir.path().join("out.mbtiles"))
        .run_failing()
        .await;

    assert!(!output.is_empty(), "the failure must be reported");
}

#[tokio::test]
async fn pack_rejects_an_unsupported_tile_extension() {
    let dir = temp_dir();
    let tree_dir = dir.path().join("bad_extension");
    fs::create_dir_all(tree_dir.join("0").join("0")).expect("failed to create a tile directory");
    fs::write(tree_dir.join("0").join("0").join("0.txt"), "not a tile")
        .expect("failed to write a tile");

    let output = MbtilesCli::new("pack")
        .arg(&tree_dir)
        .arg(dir.path().join("out.mbtiles"))
        .run_failing()
        .await;

    assert!(output.contains("Unsupported tile file extension"), "unexpected output:\n{output}");
}

#[tokio::test]
async fn pack_rejects_inconsistent_tile_formats() {
    let dir = temp_dir();
    let tree_dir = dir.path().join("mixed");
    fs::create_dir_all(tree_dir.join("0").join("0")).expect("failed to create a tile directory");
    fs::create_dir_all(tree_dir.join("1").join("0")).expect("failed to create a tile directory");
    fs::write(tree_dir.join("0").join("0").join("0.pbf"), "vector")
        .expect("failed to write a tile");
    fs::write(tree_dir.join("1").join("0").join("0.png"), "raster")
        .expect("failed to write a tile");

    let output = MbtilesCli::new("pack")
        .arg(&tree_dir)
        .arg(dir.path().join("out.mbtiles"))
        .run_failing()
        .await;

    assert!(output.contains("Inconsistent tile formats"), "unexpected output:\n{output}");
}

#[tokio::test]
async fn copy_merges_several_sources_in_order() {
    let dir = temp_dir();
    let first = mbtiles_fixture(dir.path(), "world_cities").await;
    let second = mbtiles_fixture(dir.path(), "world_cities_modified").await;
    let first_tiles = tiles(&first).await;
    let second_tiles = tiles(&second).await;
    assert_ne!(first_tiles, second_tiles, "the fixtures must differ for the order to show");

    let merged = dir.path().join("merged.mbtiles");
    MbtilesCli::new("copy")
        .arg(&first)
        .arg(&second)
        .arg(&merged)
        .arg("--on-duplicate")
        .arg("override")
        .run()
        .await;

    // Every tile of both files, the later one in `order` winning where they share a coordinate.
    let union = |order: [&Vec<Tile>; 2]| {
        let mut expected = BTreeMap::new();
        for (z, x, y, data) in order.into_iter().flatten() {
            expected.insert((*z, *x, *y), data.clone());
        }
        expected
            .into_iter()
            .map(|((z, x, y), data)| (z, x, y, data))
            .collect::<Vec<Tile>>()
    };
    assert_eq!(tiles(&merged).await, union([&first_tiles, &second_tiles]));

    let kept = dir.path().join("kept.mbtiles");
    MbtilesCli::new("copy")
        .arg(&first)
        .arg(&second)
        .arg(&kept)
        .arg("--on-duplicate")
        .arg("ignore")
        .run()
        .await;
    assert_eq!(tiles(&kept).await, union([&second_tiles, &first_tiles]));
}

#[tokio::test]
async fn copying_several_sources_needs_on_duplicate() {
    let dir = temp_dir();
    let first = mbtiles_fixture(dir.path(), "world_cities").await;
    let second = mbtiles_fixture(dir.path(), "world_cities_modified").await;
    let merged = dir.path().join("merged.mbtiles");

    let output = MbtilesCli::new("copy")
        .arg(&first)
        .arg(&second)
        .arg(&merged)
        .run_failing()
        .await;

    insta::assert_snapshot!(output, @"ERROR copying 2 sources into one file needs --on-duplicate to say what happens when two of them hold the same tile");
    assert!(!merged.exists(), "nothing is copied before the check");
}

#[tokio::test]
async fn copy_with_a_diff_file_takes_one_source() {
    let dir = temp_dir();
    let first = mbtiles_fixture(dir.path(), "world_cities").await;
    let second = mbtiles_fixture(dir.path(), "world_cities_modified").await;

    let output = MbtilesCli::new("copy")
        .arg(&first)
        .arg(&second)
        .arg(dir.path().join("merged.mbtiles"))
        .arg("--diff-with-file")
        .arg(&second)
        .arg("--on-duplicate")
        .arg("override")
        .run_failing()
        .await;

    insta::assert_snapshot!(output, @"ERROR --diff-with-file and --apply-patch take exactly one source file, got 2");
}
