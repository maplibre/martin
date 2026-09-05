//! End-to-end contour pipeline

#![cfg(feature = "contour")]

use std::assert_matches;
use std::path::PathBuf;

use martin_core::tiles::contour::{
    ContourOptions, ElevationUnits, HeightGrid, IsolineOptions, ZoomIntervalMap, generate_contours,
    trace_contours,
};
use martin_core::tiles::neighbourhood::{NEIGHBOURHOOD_LEN, Neighbourhood};
use mlt_core::fast_mvt::{MvtReaderRef, MvtValueRef};

/// The fixture tile the golden was traced from.
const ZOOM: u8 = 10;
const CENTRE_X: i32 = 163;
const CENTRE_Y: i32 = 396;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .join("tests")
        .join("fixtures")
        .join("terrain")
        .join("terrarium")
}

/// The nine Terrarium tiles around the fixture coordinate, in row-major order.
fn fixture_neighbourhood() -> Neighbourhood {
    let dir = fixtures_dir();
    let tiles: [Option<Vec<u8>>; NEIGHBOURHOOD_LEN] = std::array::from_fn(|i| {
        let (gx, gy) = (i % 3, i / 3);
        let x = CENTRE_X + i32::try_from(gx).expect("grid index") - 1;
        let y = CENTRE_Y + i32::try_from(gy).expect("grid index") - 1;
        let path = dir.join(format!("{ZOOM}_{x}_{y}.png"));
        Some(
            std::fs::read(&path)
                .map_err(|e| format!("read {}: {e}", path.display()))
                .expect("every fixture tile is committed"),
        )
    });
    Neighbourhood::from_row_major(tiles)
}

#[test]
fn the_traced_tile_decodes_to_a_stable_structure() {
    let traced = trace_contours(&fixture_neighbourhood(), ZOOM, &ContourOptions::default())
        .expect("the fixture neighbourhood traces");

    let tile = MvtReaderRef::new(&traced)
        .expect("the traced tile is valid MVT")
        .to_tile()
        .expect("the traced tile decodes");
    insta::assert_debug_snapshot!(tile.layers);
}

#[test]
fn the_traced_tile_carries_classified_contour_lines() {
    let traced = trace_contours(&fixture_neighbourhood(), ZOOM, &ContourOptions::default())
        .expect("the fixture neighbourhood traces");

    let reader = MvtReaderRef::new(&traced).expect("the traced tile is valid MVT");
    let layer = reader
        .layers()
        .find(|layer| layer.name() == "contour")
        .expect("the contour layer is present");
    assert_eq!(layer.extent(), 4096);
    assert!(layer.feature_count() > 0, "real terrain should trace at least one line");

    let mut saw_major = false;
    let mut saw_standard = false;
    let mut unexpected: Vec<String> = Vec::new();
    for feature in layer.features() {
        let tags = feature.properties_vec().expect("properties read");
        let elevation = tags
            .iter()
            .find(|(key, _)| *key == "ele")
            .map(|(_, value)| *value)
            .expect("every line carries an elevation");
        assert_matches!(
            elevation,
            MvtValueRef::UInt(_) | MvtValueRef::SInt(_),
            "elevation encodes as an integer, got {elevation:?}"
        );

        match tags
            .iter()
            .find(|(key, _)| *key == "major")
            .map(|(_, value)| *value)
        {
            Some(MvtValueRef::Bool(true)) => saw_major = true,
            Some(MvtValueRef::Bool(false)) => saw_standard = true,
            other => unexpected.push(format!("{other:?}")),
        }
    }
    assert!(unexpected.is_empty(), "every line carries a boolean major tag, got {unexpected:?}");
    assert!(
        saw_major && saw_standard,
        "the fixture spans enough relief to produce both major and minor lines"
    );
}

#[test]
fn the_filtered_threshold_is_absent_from_the_traced_tile() {
    let traced = trace_contours(&fixture_neighbourhood(), ZOOM, &ContourOptions::default())
        .expect("the fixture neighbourhood traces");

    let reader = MvtReaderRef::new(&traced).expect("the traced tile is valid MVT");
    for layer in reader.layers() {
        for feature in layer.features() {
            for r in feature.properties() {
                let (key, value) = r.expect("valid prop");
                if key == "ele"
                    && let MvtValueRef::UInt(v) = value
                {
                    assert_ne!(v, 0, "sea level should have been filtered out");
                }
            }
        }
    }
}

fn cone(side: usize) -> HeightGrid {
    #[expect(clippy::cast_precision_loss, reason = "a small test index")]
    let centre = (side / 2) as f32;
    let values = (0..side * side)
        .map(|i| {
            #[expect(clippy::cast_precision_loss, reason = "a small test index")]
            let (row, col) = ((i / side) as f32, (i % side) as f32);
            500.0 - 90.0 * (col - centre).hypot(row - centre)
        })
        .collect();
    HeightGrid::from_values(values, side, side)
}

#[test]
fn a_cone_traces_summit_rings_and_lines_that_leave_the_field() {
    let opts = IsolineOptions {
        threshold_intervals: ZoomIntervalMap::new(&[(0, 100.0)], ElevationUnits::Meters),
        simplification_tolerance: 0.0,
        min_feature_length: 0.0,
        filtered_threshold: None,
        ..IsolineOptions::default()
    };

    let traced = generate_contours(&cone(9), 0, &opts).expect("a cone traces");
    insta::assert_debug_snapshot!(traced.iter().collect::<Vec<_>>());
}
