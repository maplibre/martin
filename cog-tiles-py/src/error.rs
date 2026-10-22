//! Python exceptions for `cog-tiles` errors.

use std::io;

use cog_tiles::CogError;
use pyo3::exceptions::{PyException, PyFileNotFoundError, PyValueError};
use pyo3::prelude::*;

pyo3::create_exception!(
    cog_tiles,
    CogTilesError,
    PyException,
    "A COG or mosaic cannot be opened or read."
);
pyo3::create_exception!(
    cog_tiles,
    UnsupportedCogError,
    CogTilesError,
    "A valid TIFF that cannot be read as web map tiles, e.g. not in EPSG:3857 or not tile-aligned."
);

/// Converts a `cog-tiles` error to the closest Python exception.
#[expect(
    clippy::wildcard_enum_match_arm,
    reason = "every other error is a failure to read the data"
)]
#[expect(clippy::needless_pass_by_value, reason = "called as `map_err(to_py)`")]
pub fn to_py(error: CogError) -> PyErr {
    let message = error.to_string();
    match &error {
        CogError::Unsupported { .. } => UnsupportedCogError::new_err(message),
        CogError::InvalidTileCoord { .. }
        | CogError::InvalidBounds { .. }
        | CogError::WindowTooLarge { .. }
        | CogError::InvalidTemplate { .. }
        | CogError::FileZoomTooHigh { .. } => PyValueError::new_err(message),
        CogError::Backend { source, .. } if is_not_found(source.as_ref()) => PyFileNotFoundError::new_err(message),
        _ => CogTilesError::new_err(message),
    }
}

fn is_not_found(source: &(dyn std::error::Error + 'static)) -> bool {
    source
        .downcast_ref::<io::Error>()
        .is_some_and(|e| e.kind() == io::ErrorKind::NotFound)
        || matches!(
            source.downcast_ref::<object_store::Error>(),
            Some(object_store::Error::NotFound { .. })
        )
}
