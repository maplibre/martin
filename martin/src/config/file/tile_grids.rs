//! The `tile_grids` section, the named grids a source can be served in instead of Web Mercator.

use std::collections::{BTreeMap, HashMap};

use martin_tile_utils::{BUILT_IN_GRIDS, TileGrid, WGS1984_QUAD_ID, WORLD_CRS84_QUAD};
use serde::{Deserialize, Serialize};

use crate::config::file::file_config::declared_tile_grid;
use crate::config::file::{
    CollectUnrecognizedKeys, ConfigFileError, ConfigFileResult, FileConfigEnum, UnrecognizedValues,
};

/// The configured grids by name, as written in the config file.
pub type TileGridsConfig = BTreeMap<String, TileGridConfig>;

/// One named tile grid, a square power-of-two quad grid in a coordinate reference system.
///
/// The three values are what a tile matrix set document publishes for its zoom-0 tile.
/// They are also what `MapLibre GL JS` takes as `tileMatrix` when registering a custom projection.
#[serde_with::skip_serializing_none]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, CollectUnrecognizedKeys)]
#[cfg_attr(feature = "unstable-schemas", derive(schemars::JsonSchema))]
pub struct TileGridConfig {
    /// Coordinate reference system of the grid, as `AUTHORITY:CODE`.
    ///
    /// `EPSG` codes resolve on their own.
    /// Any other authority, such as `IAU_2015:49900` for Mars, must be present in the database's `spatial_ref_sys` table under that `auth_name` and `auth_srid`.
    #[cfg_attr(feature = "unstable-schemas", schemars(example = &"EPSG:2193"))]
    pub crs: String,

    /// Top-left corner `[x, y]` of the zoom-0 tile, in CRS units.
    #[cfg_attr(
        feature = "unstable-schemas",
        schemars(example = &[-3_260_586.728_4, 10_438_190.165_2])
    )]
    pub origin: [f64; 2],

    /// Side of the zoom-0 tile, in CRS units.
    /// Every zoom level halves it.
    #[cfg_attr(feature = "unstable-schemas", schemars(example = &10_018_754.171_4))]
    pub extent_at_zoom0: f64,

    /// How many tile `[columns, rows]` zoom 0 has \[default: `[1, 1]`\]
    ///
    /// `[2, 1]` for grids like `WorldCRS84Quad` and most planetary geographic grids, `[1, 2]` for the OGC UTM quads.
    #[cfg_attr(feature = "unstable-schemas", schemars(example = &[2u32, 1u32]))]
    pub matrix_at_zoom0: Option<[u32; 2]>,

    #[serde(flatten, skip_serializing)]
    #[cfg_attr(feature = "unstable-schemas", schemars(skip))]
    pub unrecognized: UnrecognizedValues,
}

/// Every grid a source can be served in, the [`BUILT_IN_GRIDS`] plus the configured ones.
#[derive(Clone, Debug, PartialEq)]
pub struct TileGrids(HashMap<String, TileGrid>);

impl Default for TileGrids {
    /// Just the built-in grids, [`WORLD_CRS84_QUAD`] also under its registry name [`WGS1984_QUAD_ID`].
    fn default() -> Self {
        let mut grids: HashMap<_, _> = BUILT_IN_GRIDS
            .into_iter()
            .map(|grid| (grid.id().to_owned(), grid))
            .collect();
        grids.insert(WGS1984_QUAD_ID.to_owned(), WORLD_CRS84_QUAD);
        Self(grids)
    }
}

impl TileGrids {
    /// Validates the configured grids and adds the [`BUILT_IN_GRIDS`].
    pub fn resolve(config: &TileGridsConfig) -> ConfigFileResult<Self> {
        let mut grids = Self::default();
        for (name, cfg) in config {
            if grids.0.contains_key(name) {
                return Err(ConfigFileError::TileGridRedefinesBuiltIn(name.clone()));
            }
            let mut grid = TileGrid::new(
                name.clone(),
                cfg.crs.clone(),
                cfg.origin,
                cfg.extent_at_zoom0,
            )?;
            if let Some(matrix) = cfg.matrix_at_zoom0 {
                grid = grid.with_matrix_at_zoom0(matrix)?;
            }
            grids.0.insert(name.clone(), grid);
        }
        Ok(grids)
    }

    /// The grid called `name`, if configured or built in.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&TileGrid> {
        self.0.get(name)
    }

    /// Errors if a source of a file-backed kind declares a grid it cannot have.
    ///
    /// Kinds that only produce Web Mercator tiles pass `None` for `grids`, which turns any declaration into an error.
    pub fn check_file_sources<T>(
        config: &FileConfigEnum<T>,
        grids: Option<&Self>,
    ) -> ConfigFileResult<()> {
        let FileConfigEnum::Config(cfg) = config else {
            return Ok(());
        };
        for (id, source) in cfg.sources.iter().flatten() {
            declared_tile_grid(id, source, grids)?;
        }
        Ok(())
    }

    /// Every grid name, sorted.
    #[must_use]
    pub fn names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.0.keys().map(String::as_str).collect();
        names.sort_unstable();
        names
    }
}

#[cfg(test)]
mod tests {
    use martin_tile_utils::WEB_MERCATOR_QUAD_ID;
    use rstest::rstest;

    use super::*;
    use crate::config::file::UnrecognizedKeys;

    fn dutch_rd() -> TileGridsConfig {
        serde_saphyr::from_str(
            "
DutchRD:
  crs: EPSG:28992
  origin: [-285401.92, 903401.92]
  extent_at_zoom0: 880803.84
",
        )
        .unwrap()
    }

    #[test]
    fn the_built_in_grids_are_always_there() {
        let grids = TileGrids::resolve(&TileGridsConfig::new()).unwrap();
        assert!(grids.get(WEB_MERCATOR_QUAD_ID).unwrap().is_web_mercator());
        assert_eq!(grids.get("NZTM2000Quad").unwrap().crs(), "EPSG:2193");
        assert_eq!(
            grids.get(WGS1984_QUAD_ID).unwrap(),
            grids.get("WorldCRS84Quad").unwrap()
        );
        assert_eq!(
            grids.names(),
            vec![
                "EuropeanETRS89_LAEAQuad",
                "NZTM2000Quad",
                "UPSAntarcticWGS84Quad",
                "UPSArcticWGS84Quad",
                "WGS1984Quad",
                "WebMercatorQuad",
                "WorldCRS84Quad",
                "WorldMercatorWGS84Quad",
            ]
        );
        assert_eq!(grids, TileGrids::default());
    }

    #[test]
    fn a_configured_matrix_is_applied() {
        let config: TileGridsConfig = serde_saphyr::from_str(
            "
MarsGeographic:
  crs: IAU_2015:49900
  origin: [-180, 90]
  extent_at_zoom0: 180
  matrix_at_zoom0: [2, 1]
",
        )
        .unwrap();
        let grids = TileGrids::resolve(&config).unwrap();
        assert_eq!(
            grids.get("MarsGeographic").unwrap().matrix_at_zoom0(),
            [2, 1]
        );
    }

    #[test]
    fn configured_grids_resolve_by_name() {
        let grids = TileGrids::resolve(&dutch_rd()).unwrap();
        let grid = grids.get("DutchRD").unwrap();
        assert_eq!(grid.crs(), "EPSG:28992");
        assert_eq!(
            grid.origin().map(f64::to_bits),
            [-285_401.92_f64, 903_401.92_f64].map(f64::to_bits)
        );
        assert_eq!(
            grids.names(),
            vec![
                "DutchRD",
                "EuropeanETRS89_LAEAQuad",
                "NZTM2000Quad",
                "UPSAntarcticWGS84Quad",
                "UPSArcticWGS84Quad",
                "WGS1984Quad",
                "WebMercatorQuad",
                "WorldCRS84Quad",
                "WorldMercatorWGS84Quad",
            ]
        );
        assert!(grids.get("nope").is_none());
    }

    #[rstest]
    #[case(WEB_MERCATOR_QUAD_ID)]
    #[case("NZTM2000Quad")]
    #[case(WGS1984_QUAD_ID)]
    fn a_built_in_name_cannot_be_defined_again(#[case] name: &str) {
        let mut config = dutch_rd();
        let grid = config.remove("DutchRD").unwrap();
        config.insert(name.to_owned(), grid);
        let err = TileGrids::resolve(&config).unwrap_err();
        assert!(
            matches!(&err, ConfigFileError::TileGridRedefinesBuiltIn(built_in) if built_in == name)
        );
        assert_eq!(
            err.to_string(),
            format!(
                "Tile grid {name} is built in, refer to it by name without defining it, or pick another name"
            )
        );
    }

    #[test]
    fn a_bad_grid_is_rejected_with_its_name() {
        let mut config = dutch_rd();
        config.get_mut("DutchRD").unwrap().extent_at_zoom0 = 0.0;
        let err = TileGrids::resolve(&config).unwrap_err();
        assert_eq!(
            err.to_string(),
            "tile grid DutchRD: extent_at_zoom0 must be a positive finite number, got 0"
        );
    }

    #[test]
    fn unknown_keys_are_reported() {
        let config: TileGridsConfig = serde_saphyr::from_str(
            "
g:
  crs: EPSG:4326
  origin: [-180, 90]
  extent_at_zoom0: 360
  bounds: [1, 2, 3, 4]
",
        )
        .unwrap();
        let mut keys = UnrecognizedKeys::default();
        config.collect_unrecognized("tile_grids.", &mut keys);
        assert_eq!(
            keys.into_iter().collect::<Vec<_>>(),
            vec!["tile_grids.g.bounds".to_owned()]
        );
    }
}
