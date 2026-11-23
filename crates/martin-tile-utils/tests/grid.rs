#![expect(clippy::unwrap_used, clippy::panic, reason = "tests fail by panicking")]

use std::path::Path;

use approx::relative_eq;
use martin_tile_utils::{BUILT_IN_GRIDS, TileGrid, WGS1984_QUAD_ID, WORLD_CRS84_QUAD};
use serde_json::Value;

/// The registry prints between 13 and 17 significant digits.
const REGISTRY_ROUNDING: f64 = 1e-12;

/// The registry prints the UPS scale denominators and cell sizes to 10 significant digits.
const SCALE_ROUNDING: f64 = 1e-9;

/// The 0.28 mm pixel the registry computes scale denominators for.
const STANDARD_PIXEL_METRES: f64 = 0.000_28;

/// The zoom-0 tile of a tile matrix set document, as Martin would define the grid.
struct Registered {
    id: String,
    crs: String,
    origin: [f64; 2],
    extent_at_zoom0: f64,
    matrix_at_zoom0: [u64; 2],
    ordered_axes: [String; 2],
    northing_first: bool,
    metres_per_unit: f64,
}

fn registered(id: &str) -> Registered {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures/tile-matrix-sets")
        .join(format!("{id}.json"));
    let document = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("every built-in grid has its registry document, {}: {e}", path.display()));
    let doc: Value = serde_json::from_str(&document).unwrap();
    let zoom0 = &doc["tileMatrices"][0];
    assert_eq!(zoom0["id"], "0", "{id}");
    assert_eq!(zoom0["tileWidth"], zoom0["tileHeight"], "{id}");
    let ordered_axes = [0, 1].map(|axis| doc["orderedAxes"][axis].as_str().unwrap().to_owned());
    let northing_first = matches!(ordered_axes[0].as_str(), "Y" | "N" | "Lat");
    let mut origin = [
        zoom0["pointOfOrigin"][0].as_f64().unwrap(),
        zoom0["pointOfOrigin"][1].as_f64().unwrap(),
    ];
    if northing_first {
        origin.reverse();
    }
    let cell_size = zoom0["cellSize"].as_f64().unwrap();
    let code = doc["crs"].as_str().unwrap().rsplit(['/', ':']).next().unwrap();
    Registered {
        id: doc["id"].as_str().unwrap().to_owned(),
        crs: match code {
            "CRS84" => "EPSG:4326".to_owned(),
            code => format!("EPSG:{code}"),
        },
        origin,
        extent_at_zoom0: cell_size * zoom0["tileWidth"].as_f64().unwrap(),
        matrix_at_zoom0: [
            zoom0["matrixWidth"].as_u64().unwrap(),
            zoom0["matrixHeight"].as_u64().unwrap(),
        ],
        ordered_axes,
        northing_first,
        metres_per_unit: zoom0["scaleDenominator"].as_f64().unwrap() * STANDARD_PIXEL_METRES / cell_size,
    }
}

fn assert_same_grid(grid: &TileGrid, registered: &Registered) {
    let id = &registered.id;
    assert_eq!(grid.crs(), registered.crs, "{id}");
    assert!(
        relative_eq!(grid.origin()[0], registered.origin[0], max_relative = REGISTRY_ROUNDING)
            && relative_eq!(grid.origin()[1], registered.origin[1], max_relative = REGISTRY_ROUNDING),
        "{id}: origin {:?}, the registry has {:?}",
        grid.origin(),
        registered.origin
    );
    assert!(
        relative_eq!(
            grid.extent_at_zoom0(),
            registered.extent_at_zoom0,
            max_relative = REGISTRY_ROUNDING
        ),
        "{id}: extent at zoom 0 {}, the registry has {}",
        grid.extent_at_zoom0(),
        registered.extent_at_zoom0
    );
    assert_eq!(
        grid.matrix_at_zoom0().map(u64::from),
        registered.matrix_at_zoom0,
        "{id}"
    );
}

#[test]
fn every_built_in_grid_is_its_registry_document() {
    for grid in &BUILT_IN_GRIDS {
        let registered = registered(grid.id());
        assert_eq!(registered.id, grid.id());
        assert_same_grid(grid, &registered);
        let axes = grid.crs_axes().unwrap();
        assert_eq!(axes.ordered_axes, registered.ordered_axes, "{}", grid.id());
        assert_eq!(axes.northing_first, registered.northing_first, "{}", grid.id());
        assert!(
            relative_eq!(
                axes.metres_per_unit,
                registered.metres_per_unit,
                max_relative = SCALE_ROUNDING
            ),
            "{}: {} metres per unit, the registry has {}",
            grid.id(),
            axes.metres_per_unit,
            registered.metres_per_unit
        );
    }
}

#[test]
fn the_registry_files_wgs1984quad_as_world_crs84quad_in_epsg_4326_axis_order() {
    let registered = registered(WGS1984_QUAD_ID);
    assert_eq!(registered.id, WORLD_CRS84_QUAD.id());
    assert_same_grid(&WORLD_CRS84_QUAD, &registered);
}
