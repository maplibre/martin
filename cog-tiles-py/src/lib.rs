//! Python bindings for [`cog_tiles`]: web map tiles and stitched rasters from tile-aligned COGs.

mod error;
mod location;
mod py;
mod runtime;
mod source;

use pyo3::prelude::*;
use pyo3_stub_gen::{define_stub_info_gatherer, module_variable};

/// Web map tiles and stitched rasters from tile-aligned Cloud Optimized `GeoTIFFs`.
#[pymodule(name = "cog_tiles")]
fn _cog_tiles(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(py::open, m)?)?;
    m.add_function(wrap_pyfunction!(py::open_mosaic, m)?)?;
    m.add_function(wrap_pyfunction!(py::open_async, m)?)?;
    m.add_function(wrap_pyfunction!(py::open_mosaic_async, m)?)?;
    m.add_function(wrap_pyfunction!(py::open_tile_index, m)?)?;
    m.add_function(wrap_pyfunction!(py::open_tile_index_async, m)?)?;
    m.add_class::<py::TileSource>()?;
    m.add_class::<py::Cog>()?;
    m.add_class::<py::Mosaic>()?;
    m.add_class::<py::TileIndex>()?;
    m.add_class::<py::Raster>()?;
    m.add_class::<py::TileIterator>()?;
    m.add("CogTilesError", m.py().get_type::<error::CogTilesError>())?;
    m.add("UnsupportedCogError", m.py().get_type::<error::UnsupportedCogError>())?;
    m.add("MAX_ZOOM", cog_tiles::MAX_ZOOM)?;
    Ok(())
}

module_variable!("cog_tiles", "MAX_ZOOM", u8);

define_stub_info_gatherer!(stub_info);
