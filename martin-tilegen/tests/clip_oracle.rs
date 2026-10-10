//! Differential test of the renderer's tile coverage against martin-core's float clipping.
//! The renderer quantizes before slicing and the oracle clips before quantizing, so near a buffered
//! edge they may disagree by a unit: every tile the oracle finds with the buffer shrunk by one must be
//! rendered, and every rendered tile must be found by the oracle with the buffer grown by one.
#![expect(clippy::unwrap_used)]

use std::collections::BTreeSet;
use std::num::NonZeroU32;

use geo_types::{Coord, Geometry, LineString, MultiPoint, Point};
use martin_core::tiles::geojson::clip_to_tile;
use martin_tile_utils::EARTH_CIRCUMFERENCE;
use martin_tilegen::record::EncodedProps;
use martin_tilegen::{
    Feature, FeatureGeom, LayerGrid, PixelThreshold, RenderLayer, Renderer, Seq, SortConfig,
    Sorter, TileOrder,
};

const GRID: LayerGrid = LayerGrid {
    extent: 256,
    buffer: 8,
};
const MAX_ZOOM: u8 = 5;

type Tiles = BTreeSet<(u8, u32, u32)>;

fn rendered(geom: FeatureGeom<'_>) -> Tiles {
    let dir = tempfile::tempdir().unwrap();
    let sorter = Sorter::new(SortConfig {
        temp_dirs: vec![dir.path().to_path_buf()],
        buffer_bytes: 1 << 20,
        max_fan_in: 8,
        read_buffer_bytes: 4096,
    })
    .unwrap();
    let layer = RenderLayer::new(0, 0..=MAX_ZOOM, GRID).unwrap();
    let mut buffer = sorter.buffer();
    let feature = Feature {
        id: None,
        geom,
        props: &EncodedProps::default(),
        zooms: 0..=MAX_ZOOM,
        simplify: PixelThreshold::ZERO,
        min_size: PixelThreshold::ZERO,
    };
    Renderer::default()
        .render(
            TileOrder::Tms,
            &layer,
            Seq::default(),
            &feature,
            &mut buffer,
        )
        .unwrap();
    buffer.finish().unwrap();
    let mut merger = sorter.merge().unwrap();
    let mut tiles = Tiles::new();
    while let Some((key, _)) = merger.next_record().unwrap() {
        let c = TileOrder::Tms.tile_coord(key.tile_id()).unwrap();
        tiles.insert((c.z(), c.x(), c.y()));
    }
    tiles
}

fn oracle(geom: &Geometry<f64>, buffer: u32) -> Tiles {
    let extent = NonZeroU32::new(GRID.extent).unwrap();
    let mut tiles = Tiles::new();
    for z in 0..=MAX_ZOOM {
        for x in 0..1 << z {
            for y in 0..1 << z {
                if clip_to_tile(geom.clone(), (z, x, y), extent, buffer).is_some() {
                    tiles.insert((z, x, y));
                }
            }
        }
    }
    tiles
}

/// Unit coordinates (y down) to EPSG:3857 meters (y up), the oracle's space.
fn meters(c: Coord<f64>) -> Coord<f64> {
    Coord {
        x: (c.x - 0.5) * EARTH_CIRCUMFERENCE,
        y: (0.5 - c.y) * EARTH_CIRCUMFERENCE,
    }
}

fn check(name: &str, ours: &Tiles, geom: &Geometry<f64>) {
    let inner = oracle(geom, GRID.buffer - 1);
    let outer = oracle(geom, GRID.buffer + 1);
    assert!(
        inner.is_subset(ours),
        "{name}: missing tiles {:?}",
        inner.difference(ours).collect::<Vec<_>>()
    );
    assert!(
        ours.is_subset(&outer),
        "{name}: extra tiles {:?}",
        ours.difference(&outer).collect::<Vec<_>>()
    );
}

fn random_coords(seed: u64, n: usize) -> Vec<Coord<f64>> {
    let mut state = seed;
    let mut next = move || {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        0.05 + 0.9 * f64::from(u32::try_from(state >> 40).unwrap()) / f64::from(1u32 << 24)
    };
    (0..n)
        .map(|_| Coord {
            x: next(),
            y: next(),
        })
        .collect()
}

#[test]
fn lines_cover_the_same_tiles() {
    for seed in 0..12 {
        let coords = random_coords(seed, 2 + usize::try_from(seed % 5).unwrap());
        let line = LineString::new(coords);
        let ours = rendered(FeatureGeom::Lines(std::slice::from_ref(&line)));
        let geom = Geometry::LineString(LineString::new(
            line.0.iter().copied().map(meters).collect(),
        ));
        check(&format!("line {seed}"), &ours, &geom);
    }
}

#[test]
fn points_cover_the_same_tiles() {
    for seed in 100..112 {
        let coords = random_coords(seed, 3);
        let ours = rendered(FeatureGeom::Points(&coords));
        let geom = Geometry::MultiPoint(MultiPoint(
            coords.iter().map(|&c| Point(meters(c))).collect(),
        ));
        check(&format!("points {seed}"), &ours, &geom);
    }
}
