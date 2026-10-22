//! Python bindings for [`cog_tiles`]: web map tiles and stitched rasters from tile-aligned COGs.

mod error;
mod location;
mod py;
mod runtime;
mod source;

use pyo3::prelude::*;

#[pymodule]
fn _cog_tiles(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(py::open, m)?)?;
    m.add_function(wrap_pyfunction!(py::open_mosaic, m)?)?;
    m.add_class::<py::TileSource>()?;
    m.add_class::<py::Cog>()?;
    m.add_class::<py::Mosaic>()?;
    m.add_class::<py::Raster>()?;
    m.add_class::<py::TileIterator>()?;
    m.add("CogTilesError", m.py().get_type::<error::CogTilesError>())?;
    m.add("UnsupportedCogError", m.py().get_type::<error::UnsupportedCogError>())?;
    m.add("MAX_ZOOM", cog_tiles::MAX_ZOOM)?;
    Ok(())
}
