//! End-to-end contour pipeline

#![cfg(feature = "contour")]

use std::assert_matches;
use std::path::PathBuf;

use martin_core::tiles::contour::{
    ContourOptions, ElevationUnits, HeightGrid, IsolineOptions, ZoomIntervalMap, generate_contours,
    trace_contours,
};
use martin_core::tiles::neighbourhood::{DEFAULT_TILE_SIZE, NEIGHBOURHOOD_LEN, Neighbourhood};
use martin_tile_utils::TileData;
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
    let tiles: [Option<TileData>; NEIGHBOURHOOD_LEN] = std::array::from_fn(|i| {
        let (gx, gy) = (i % 3, i / 3);
        let x = CENTRE_X + i32::try_from(gx).expect("grid index") - 1;
        let y = CENTRE_Y + i32::try_from(gy).expect("grid index") - 1;
        let path = dir.join(format!("{ZOOM}_{x}_{y}.png"));
        Some(TileData::from(
            std::fs::read(&path)
                .map_err(|e| format!("read {}: {e}", path.display()))
                .expect("every fixture tile is committed"),
        ))
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
    assert!(
        layer.feature_count() > 0,
        "real terrain should trace at least one line"
    );

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
    assert!(
        unexpected.is_empty(),
        "every line carries a boolean major tag, got {unexpected:?}"
    );
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

/// A 512-square source, twice the side the pipeline used to assume.
const OVERSIZED_TILE: usize = 2 * DEFAULT_TILE_SIZE;

/// Elevation in meters at `(gx, gy)` of the 3x3 ramp, in whole-field pixels.
fn ramp_meters(gx: usize, gy: usize) -> f32 {
    #[expect(clippy::cast_precision_loss, reason = "a field coordinate is small")]
    let sum = (gx + gy) as f32;
    #[expect(clippy::cast_precision_loss, reason = "a tile side is small")]
    let span = (3 * OVERSIZED_TILE) as f32;
    sum * 1200.0 / span
}

/// PNG-encodes one `OVERSIZED_TILE`-square Terrarium tile of the ramp, with
/// `(grid_x, grid_y)` naming its cell in the 3x3 neighbourhood.
fn ramp_tile(grid_x: usize, grid_y: usize) -> TileData {
    let mut pixels = Vec::with_capacity(OVERSIZED_TILE * OVERSIZED_TILE * 4);
    for y in 0..OVERSIZED_TILE {
        for x in 0..OVERSIZED_TILE {
            let raw =
                ramp_meters(grid_x * OVERSIZED_TILE + x, grid_y * OVERSIZED_TILE + y) + 32768.0;
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "the ramp stays inside the Terrarium range"
            )]
            let r = (raw / 256.0).floor() as u8;
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "the ramp stays inside the Terrarium range"
            )]
            let g = (raw - f32::from(r) * 256.0).floor() as u8;
            pixels.extend_from_slice(&[r, g, 0, 255]);
        }
    }

    let mut buf = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_raw(
        u32::try_from(OVERSIZED_TILE).expect("tile side"),
        u32::try_from(OVERSIZED_TILE).expect("tile side"),
        pixels,
    )
    .expect("the buffer matches the tile side")
    .write_to(&mut buf, image::ImageFormat::Png)
    .expect("encode the ramp tile");
    buf.into_inner().into()
}

/// Rounds an elevation to the whole meters a traced feature reports.
#[expect(
    clippy::cast_possible_truncation,
    reason = "the ramp stays inside the Terrarium range"
)]
fn whole_meters(meters: f32) -> i64 {
    meters.round() as i64
}

/// The whole-meter elevation a traced feature carries, if it encodes as an integer.
#[expect(
    clippy::wildcard_enum_match_arm,
    reason = "every non-integer value is equally wrong here"
)]
fn elevation_of(value: MvtValueRef<'_>) -> Option<i64> {
    match value {
        MvtValueRef::UInt(v) => i64::try_from(v).ok(),
        MvtValueRef::SInt(v) => Some(v),
        _ => None,
    }
}

/// A neighbourhood of `OVERSIZED_TILE`-square tiles carrying one unbroken
/// diagonal elevation ramp across all nine of them.
fn oversized_ramp_neighbourhood() -> Neighbourhood {
    Neighbourhood::from_row_major(std::array::from_fn(|i| Some(ramp_tile(i % 3, i / 3))))
}

/// Traced tile-space segments that run along one of the four tile edges.
fn edge_hugging_segments(traced: &[u8]) -> Vec<(geo_types::Coord<i32>, geo_types::Coord<i32>)> {
    const EXTENT: f64 = 4096.0;
    const TOLERANCE: f64 = 24.0;
    let on_edge = |v: f64| v.abs() <= TOLERANCE || (v - EXTENT).abs() <= TOLERANCE;

    let tile = MvtReaderRef::new(traced)
        .expect("the traced tile is valid MVT")
        .to_tile()
        .expect("the traced tile decodes");

    let mut segments = Vec::new();
    for layer in &tile.layers {
        for feature in &layer.features {
            let geo_types::Geometry::LineString(line) = &feature.geometry else {
                continue;
            };
            for pair in line.0.windows(2) {
                let (a, b) = (pair[0], pair[1]);
                let (ax, ay) = (f64::from(a.x), f64::from(a.y));
                let (bx, by) = (f64::from(b.x), f64::from(b.y));
                let along_x = (bx - ax).abs() <= 2.0 && on_edge(ax) && on_edge(bx);
                let along_y = (by - ay).abs() <= 2.0 && on_edge(ay) && on_edge(by);
                if along_x || along_y {
                    segments.push((a, b));
                }
            }
        }
    }
    segments
}

#[test]
fn an_oversized_source_traces_no_contours_along_the_tile_edges() {
    let traced = trace_contours(
        &oversized_ramp_neighbourhood(),
        ZOOM,
        &ContourOptions::default(),
    )
    .expect("the ramp neighbourhood traces");

    let segments = edge_hugging_segments(&traced);
    assert!(
        segments.is_empty(),
        "an unbroken ramp has no contour running along a tile edge, found {}: {:?}",
        segments.len(),
        &segments[..segments.len().min(4)]
    );
}

#[test]
fn an_oversized_source_traces_its_whole_tile() {
    let traced = trace_contours(
        &oversized_ramp_neighbourhood(),
        ZOOM,
        &ContourOptions::default(),
    )
    .expect("the ramp neighbourhood traces");

    let centre_low = whole_meters(ramp_meters(OVERSIZED_TILE, OVERSIZED_TILE));
    let centre_high = whole_meters(ramp_meters(2 * OVERSIZED_TILE, 2 * OVERSIZED_TILE));
    let apron = 100;

    let reader = MvtReaderRef::new(&traced).expect("the traced tile is valid MVT");
    let mut lowest = i64::MAX;
    let mut highest = i64::MIN;
    for layer in reader.layers() {
        for feature in layer.features() {
            for property in feature.properties() {
                let (key, value) = property.expect("valid property");
                if key != "ele" {
                    continue;
                }
                let elevation = elevation_of(value).expect("an elevation encodes as an integer");
                lowest = lowest.min(elevation);
                highest = highest.max(elevation);
            }
        }
    }

    assert!(
        lowest >= centre_low - apron,
        "traced {lowest} m, below the centre tile's floor of {centre_low} m"
    );
    assert!(
        highest >= centre_high - apron,
        "traced up to {highest} m, short of the centre tile's ceiling of {centre_high} m"
    );
}
