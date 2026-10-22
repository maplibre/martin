//! The Python classes and functions.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use cog_tiles::{MosaicOptions, PixelWindow, TileCoord, lnglat_to_mercator};
use numpy::ndarray::Array3;
use numpy::{IntoPyArray as _, PyArray3, PyUntypedArrayMethods as _};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyBytes;
use pyo3_stub_gen::derive::{gen_stub_pyclass, gen_stub_pyfunction, gen_stub_pymethods};
use tokio::sync::{Mutex, mpsc};

use crate::error::to_py;
use crate::location::{Location, split_mosaic};
use crate::runtime::{AbortOnDrop, runtime, wait};
use crate::source::{Layout, Source, read_window, stream_tiles};

/// How many tiles are fetched and decoded at once by default.
const DEFAULT_CONCURRENCY: usize = 64;

/// The largest raster `read` returns by default, in pixels: 16384 x 16384.
const DEFAULT_MAX_PIXELS: u64 = 1 << 28;

/// Tiles read from one COG or from a mosaic of them.
#[gen_stub_pyclass]
#[pyclass(frozen, subclass, module = "cog_tiles")]
pub struct TileSource {
    source: Arc<Source>,
    layout: Layout,
}

/// One zoom, or several.
#[derive(FromPyObject)]
enum Zooms {
    One(u8),
    Many(Vec<u8>),
}

#[gen_stub_pymethods]
#[pymethods]
impl TileSource {
    /// The lowest zoom with tiles.
    #[getter]
    fn min_zoom(&self) -> u8 {
        self.layout.min_zoom
    }

    /// The highest zoom with tiles.
    #[getter]
    fn max_zoom(&self) -> u8 {
        self.layout.max_zoom
    }

    /// Tile width and height in pixels.
    #[getter]
    fn tile_size(&self) -> u32 {
        self.layout.tile_size
    }

    /// The format of every tile: `"jpg"`, `"webp"` or `"png"`.
    #[getter]
    #[gen_stub(override_return_type(type_repr = "typing.Literal['jpg', 'webp', 'png']", imports = ("typing")))]
    fn format(&self) -> &'static str {
        self.layout.format.extension()
    }

    /// The MIME type of every tile, e.g. `"image/jpeg"`.
    #[getter]
    fn content_type(&self) -> &'static str {
        self.layout.format.content_type()
    }

    /// `(west, south, east, north)` in degrees, or `None` for a mosaic.
    #[getter]
    fn bounds(&self) -> Option<(f64, f64, f64, f64)> {
        self.source.bounds().map(|(bounds, _)| bounds.into())
    }

    /// `(min_x, min_y, max_x, max_y)` in EPSG:3857 meters, or `None` for a mosaic.
    #[getter]
    fn mercator_bounds(&self) -> Option<(f64, f64, f64, f64)> {
        self.source.bounds().map(|(_, bounds)| bounds.into())
    }

    /// The encoded tile `z/x/y` (XYZ, `y = 0` at the top), or `None` if there is no data there.
    fn tile<'py>(&self, py: Python<'py>, z: u8, x: u32, y: u32) -> PyResult<Option<Bound<'py, PyBytes>>> {
        let coord = TileCoord::new(z, x, y).map_err(to_py)?;
        let tile = wait(py, self.source.tile(coord))?.map_err(to_py)?;
        Ok(tile.map(|data| PyBytes::new(py, &data)))
    }

    /// The decoded tile `z/x/y` as a `(tile_size, tile_size, bands)` `uint8` array,
    /// or `None` if there is no data there.
    fn tile_pixels<'py>(&self, py: Python<'py>, z: u8, x: u32, y: u32) -> PyResult<Option<Bound<'py, PyArray3<u8>>>> {
        let coord = TileCoord::new(z, x, y).map_err(to_py)?;
        let source = Arc::clone(&self.source);
        let pixels = wait(py, async move { source.pixels(coord).await })?.map_err(to_py)?;
        pixels
            .map(|pixels| {
                let size = pixels.size() as usize;
                let shape = (size, size, pixels.bands());
                to_array(py, shape, pixels.into_data())
            })
            .transpose()
    }

    /// Iterates over the encoded tiles with data in `bbox` at `zoom`, as `(z, x, y, data)`,
    /// fetching `concurrency` at a time in the background in whatever order they arrive.
    ///
    /// `zoom` is one zoom or a sequence of them, all zooms of the source when `None`.
    /// `bbox` is the source's bounds when `None`; a mosaic needs one.
    #[pyo3(signature = (bbox = None, zoom = None, *, crs = "EPSG:4326", concurrency = DEFAULT_CONCURRENCY))]
    fn tiles(
        &self,
        #[gen_stub(override_type(type_repr = "tuple[builtins.float, builtins.float, builtins.float, builtins.float] | None", imports = ("builtins")))]
        bbox: Option<[f64; 4]>,
        #[gen_stub(override_type(type_repr = "builtins.int | typing.Sequence[builtins.int] | None", imports = ("builtins", "typing")))]
        zoom: Option<Zooms>,
        #[gen_stub(override_type(type_repr = "typing.Literal['EPSG:4326', 'EPSG:3857']", imports = ("typing")))]
        crs: &str,
        concurrency: usize,
    ) -> PyResult<TileIterator> {
        check_concurrency(concurrency)?;
        let mut bounds = match (bbox, self.source.bounds()) {
            (Some(bbox), _) => to_mercator(bbox, crs)?,
            (None, Some((_, mercator))) => mercator,
            (None, None) => return Err(PyValueError::new_err("a mosaic needs a bbox")),
        };
        if let Some((_, [min_x, min_y, max_x, max_y])) = self.source.bounds() {
            bounds = [
                bounds[0].max(min_x),
                bounds[1].max(min_y),
                bounds[2].min(max_x),
                bounds[3].min(max_y),
            ];
        }
        let zooms = match zoom {
            Some(Zooms::One(zoom)) => vec![zoom],
            Some(Zooms::Many(zooms)) => zooms,
            None => (self.layout.min_zoom..=self.layout.max_zoom).collect(),
        };
        let mut windows = Vec::new();
        if bounds[0] < bounds[2] && bounds[1] < bounds[3] {
            for zoom in zooms {
                if (self.layout.min_zoom..=self.layout.max_zoom).contains(&zoom) {
                    windows
                        .push(PixelWindow::from_mercator_bounds(zoom, self.layout.tile_size, bounds).map_err(to_py)?);
                }
            }
        }
        let (sender, receiver) = mpsc::channel(concurrency);
        let task = runtime()?.spawn(stream_tiles(Arc::clone(&self.source), windows, concurrency, sender));
        Ok(TileIterator {
            receiver: Mutex::new(receiver),
            _task: AbortOnDrop(task),
        })
    }

    /// Stitches the decoded tiles that `bbox` touches at `zoom` into one raster of whole pixels,
    /// fetching and decoding `concurrency` tiles at a time on all cores.
    ///
    /// `zoom` is the highest zoom of the source when `None`. `bbox` is in `crs`, `"EPSG:4326"`
    /// (degrees) or `"EPSG:3857"` (meters). Pixels without data are 0.
    /// Returns `None` if no tile in `bbox` has data.
    #[pyo3(signature = (
        bbox,
        zoom = None,
        *,
        crs = "EPSG:4326",
        concurrency = DEFAULT_CONCURRENCY,
        max_pixels = DEFAULT_MAX_PIXELS,
    ))]
    fn read(
        &self,
        py: Python<'_>,
        #[gen_stub(override_type(type_repr = "tuple[builtins.float, builtins.float, builtins.float, builtins.float]", imports = ("builtins")))]
        bbox: [f64; 4],
        zoom: Option<u8>,
        #[gen_stub(override_type(type_repr = "typing.Literal['EPSG:4326', 'EPSG:3857']", imports = ("typing")))]
        crs: &str,
        concurrency: usize,
        max_pixels: u64,
    ) -> PyResult<Option<Raster>> {
        check_concurrency(concurrency)?;
        let zoom = zoom.unwrap_or(self.layout.max_zoom);
        let window =
            PixelWindow::from_mercator_bounds(zoom, self.layout.tile_size, to_mercator(bbox, crs)?).map_err(to_py)?;
        let pixels = u64::from(window.width()) * u64::from(window.height());
        if pixels > max_pixels {
            return Err(PyValueError::new_err(format!(
                "the raster would be {}x{} pixels, more than max_pixels={max_pixels}; use a lower zoom, a smaller bbox, or raise max_pixels",
                window.width(),
                window.height(),
            )));
        }
        let Some((data, bands)) = wait(py, read_window(Arc::clone(&self.source), window, concurrency))?? else {
            return Ok(None);
        };
        let shape = (window.height() as usize, window.width() as usize, bands);
        Ok(Some(Raster {
            data: to_array(py, shape, data)?.unbind(),
            transform: window.geotransform(),
            bounds: window.mercator_bounds(),
            zoom,
        }))
    }
}

/// One tile-aligned Cloud Optimized `GeoTIFF`.
#[gen_stub_pyclass]
#[pyclass(frozen, extends = TileSource, module = "cog_tiles")]
pub struct Cog {
    location: PathBuf,
    options: BTreeMap<String, String>,
}

#[gen_stub_pymethods]
#[pymethods]
impl Cog {
    #[gen_stub(skip)]
    fn __reduce__<'py>(slf: &Bound<'py, Self>) -> PyResult<(Bound<'py, PyAny>, CogArgs)> {
        let this = slf.get();
        let open = slf.py().import("cog_tiles")?.getattr("open")?;
        Ok((open, (this.location.clone(), this.options.clone())))
    }

    fn __repr__(slf: &Bound<'_, Self>) -> String {
        let layout = slf.as_super().get().layout;
        format!(
            "Cog({:?}, zooms={}..={}, tile_size={}, format={:?})",
            slf.get().location.display().to_string(),
            layout.min_zoom,
            layout.max_zoom,
            layout.tile_size,
            layout.format.extension(),
        )
    }
}

/// The arguments `open` is called with to restore a pickled COG.
type CogArgs = (PathBuf, BTreeMap<String, String>);

/// Many COGs, each covering one tile at `file_zoom` and named after it.
#[gen_stub_pyclass]
#[pyclass(frozen, extends = TileSource, module = "cog_tiles")]
pub struct Mosaic {
    location: String,
    options: MosaicOptions,
    store_options: BTreeMap<String, String>,
}

/// The arguments `open_mosaic` is called with to restore a pickled mosaic.
type MosaicArgs = (String, u8, u64, f64, Option<f64>, BTreeMap<String, String>);

#[gen_stub_pymethods]
#[pymethods]
impl Mosaic {
    /// The zoom at which each file covers exactly one tile.
    #[getter]
    fn file_zoom(&self) -> u8 {
        self.options.file_zoom
    }

    /// The file name template, e.g. `"{quadkey}.tif"`.
    #[getter]
    fn template(slf: &Bound<'_, Self>) -> String {
        split_mosaic(&slf.get().location).1.to_owned()
    }

    #[gen_stub(skip)]
    fn __reduce__<'py>(slf: &Bound<'py, Self>) -> PyResult<(Bound<'py, PyAny>, MosaicArgs)> {
        let this = slf.get();
        let open = slf.py().import("cog_tiles")?.getattr("open_mosaic")?;
        let options = this.options;
        Ok((
            open,
            (
                this.location.clone(),
                options.file_zoom,
                options.max_cached_files,
                options.file_expiry.as_secs_f64(),
                options.file_idle_timeout.map(|d| d.as_secs_f64()),
                this.store_options.clone(),
            ),
        ))
    }

    fn __repr__(slf: &Bound<'_, Self>) -> String {
        let layout = slf.as_super().get().layout;
        format!(
            "Mosaic({:?}, file_zoom={}, max_zoom={}, tile_size={}, format={:?})",
            slf.get().location,
            layout.min_zoom,
            layout.max_zoom,
            layout.tile_size,
            layout.format.extension(),
        )
    }
}

/// Pixels stitched from tiles, with where they are in EPSG:3857.
///
/// `numpy.asarray(raster)` is `raster.data`.
#[gen_stub_pyclass]
#[pyclass(frozen, module = "cog_tiles")]
pub struct Raster {
    data: Py<PyArray3<u8>>,
    transform: [f64; 6],
    bounds: [f64; 4],
    zoom: u8,
}

#[gen_stub_pymethods]
#[pymethods]
impl Raster {
    /// The pixels as a `(height, width, bands)` `uint8` array.
    #[getter]
    fn data(&self, py: Python<'_>) -> Py<PyArray3<u8>> {
        self.data.clone_ref(py)
    }

    /// The pixel to EPSG:3857 transform in GDAL order:
    /// `(min_x, resolution, 0, max_y, 0, -resolution)`.
    /// For rasterio: `affine.Affine.from_gdal(*raster.transform)`.
    #[getter]
    fn transform(&self) -> (f64, f64, f64, f64, f64, f64) {
        self.transform.into()
    }

    /// `(min_x, min_y, max_x, max_y)` in EPSG:3857 meters, snapped outward to whole pixels.
    #[getter]
    fn bounds(&self) -> (f64, f64, f64, f64) {
        self.bounds.into()
    }

    /// The zoom level the pixels are at.
    #[getter]
    fn zoom(&self) -> u8 {
        self.zoom
    }

    /// Always `"EPSG:3857"`.
    #[getter]
    #[expect(clippy::unused_self, reason = "a Python property")]
    #[gen_stub(override_return_type(type_repr = "typing.Literal['EPSG:3857']", imports = ("typing")))]
    fn crs(&self) -> &'static str {
        "EPSG:3857"
    }

    #[pyo3(signature = (dtype = None, copy = None))]
    #[gen_stub(override_return_type(type_repr = "numpy.ndarray", imports = ("numpy")))]
    fn __array__<'py>(
        &self,
        py: Python<'py>,
        dtype: Option<Bound<'py, PyAny>>,
        copy: Option<bool>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let array = self.data.bind(py).clone().into_any();
        match (dtype, copy) {
            (Some(dtype), _) => array.call_method1("astype", (dtype,)),
            (None, Some(true)) => array.call_method0("copy"),
            (None, _) => Ok(array),
        }
    }

    fn __repr__(&self, py: Python<'_>) -> String {
        let shape = self.data.bind(py).shape().to_vec();
        format!("Raster(shape={shape:?}, zoom={}, bounds={:?})", self.zoom, self.bounds)
    }
}

/// One tile from a [`TileIterator`]: `(z, x, y, data)`.
type TileItem<'py> = (u8, u32, u32, Bound<'py, PyBytes>);

/// Encoded tiles as `(z, x, y, data)`, fetched in the background. Dropping it stops the fetches.
#[gen_stub_pyclass]
#[pyclass(frozen, module = "cog_tiles")]
pub struct TileIterator {
    receiver: Mutex<mpsc::Receiver<PyResult<(TileCoord, bytes::Bytes)>>>,
    _task: AbortOnDrop<()>,
}

#[gen_stub_pymethods]
#[pymethods]
impl TileIterator {
    fn __iter__(slf: Py<Self>) -> Py<Self> {
        slf
    }

    #[gen_stub(override_return_type(type_repr = "tuple[builtins.int, builtins.int, builtins.int, builtins.bytes]", imports = ("builtins")))]
    fn __next__<'py>(&self, py: Python<'py>) -> PyResult<Option<TileItem<'py>>> {
        let next = wait(py, async { self.receiver.lock().await.recv().await })?;
        next.transpose()?
            .map(|(coord, data)| Ok((coord.z(), coord.x(), coord.y(), PyBytes::new(py, &data))))
            .transpose()
    }
}

/// Opens one COG from a local path or a URL (`s3://`, `gs://`, `az://`, `https://`, `file://`).
///
/// Object store credentials come from the usual environment variables (e.g. `AWS_REGION`,
/// `AWS_ACCESS_KEY_ID`), overridden by `options` (e.g. `{"aws_skip_signature": "true"}`).
#[gen_stub_pyfunction]
#[pyfunction]
#[pyo3(signature = (location, options = None))]
pub fn open(py: Python<'_>, location: PathBuf, options: Option<BTreeMap<String, String>>) -> PyResult<Py<Cog>> {
    let options = options.unwrap_or_default();
    let parsed = parse_location(&location, &options)?;
    let source = wait(py, Source::open(parsed))?.map_err(to_py)?;
    let base = TileSource {
        layout: source.layout(),
        source: Arc::new(source),
    };
    Py::new(
        py,
        PyClassInitializer::from(base).add_subclass(Cog { location, options }),
    )
}

/// Opens a mosaic of COGs that each cover one tile at `file_zoom`, named by a template in
/// `location`: `{quadkey}`, or `{x}` and `{y}` with an optional `{z}`, e.g.
/// `"s3://bucket/ortho/{quadkey}.tif"` or `"/data/ortho/{z}/{x}/{y}.tif"`.
/// `{-y}` is the row counted from the bottom (TMS).
///
/// One file is found by listing the location now; the others are opened on first use and kept
/// for `file_expiry` seconds (or `file_idle_timeout` seconds unused), at most `max_cached_files`.
#[gen_stub_pyfunction]
#[pyfunction]
#[pyo3(signature = (
    location,
    file_zoom,
    max_cached_files = cog_tiles::DEFAULT_MAX_CACHED_FILES,
    file_expiry = cog_tiles::DEFAULT_FILE_EXPIRY.as_secs_f64(),
    file_idle_timeout = None,
    options = None,
))]
pub fn open_mosaic(
    py: Python<'_>,
    location: String,
    file_zoom: u8,
    max_cached_files: u64,
    file_expiry: f64,
    file_idle_timeout: Option<f64>,
    options: Option<BTreeMap<String, String>>,
) -> PyResult<Py<Mosaic>> {
    let store_options = options.unwrap_or_default();
    let seconds = |value: f64| {
        Duration::try_from_secs_f64(value)
            .map_err(|e| PyValueError::new_err(format!("{value} is not a valid number of seconds: {e}")))
    };
    let options = MosaicOptions::new(file_zoom)
        .with_max_cached_files(max_cached_files)
        .with_file_expiry(seconds(file_expiry)?)
        .with_file_idle_timeout(file_idle_timeout.map(seconds).transpose()?);
    let (root, template) = split_mosaic(&location);
    let pairs: Vec<_> = store_options.clone().into_iter().collect();
    let parsed = Location::parse(&root, &pairs)?;
    let source = wait(py, Source::open_mosaic(parsed, template, options))?.map_err(to_py)?;
    let base = TileSource {
        layout: source.layout(),
        source: Arc::new(source),
    };
    let mosaic = Mosaic {
        location,
        options,
        store_options,
    };
    Py::new(py, PyClassInitializer::from(base).add_subclass(mosaic))
}

fn parse_location(location: &std::path::Path, options: &BTreeMap<String, String>) -> PyResult<Location> {
    match location.to_str() {
        Some(location) => {
            let pairs: Vec<_> = options.clone().into_iter().collect();
            Location::parse(location, &pairs)
        }
        None => Ok(Location::Local(location.to_path_buf())),
    }
}

fn to_mercator(bbox: [f64; 4], crs: &str) -> PyResult<[f64; 4]> {
    let [a, b, c, d] = bbox;
    match crs.to_ascii_uppercase().as_str() {
        "EPSG:4326" | "WGS84" => {
            if !bbox.iter().all(|v| v.is_finite()) || a >= c || b >= d {
                return Err(to_py(cog_tiles::CogError::InvalidBounds { bounds: bbox }));
            }
            let [min_x, min_y] = lnglat_to_mercator(a, b);
            let [max_x, max_y] = lnglat_to_mercator(c, d);
            Ok([min_x, min_y, max_x, max_y])
        }
        "EPSG:3857" => Ok(bbox),
        _ => Err(PyValueError::new_err(format!(
            "crs must be \"EPSG:4326\" or \"EPSG:3857\", not {crs:?}"
        ))),
    }
}

fn check_concurrency(concurrency: usize) -> PyResult<()> {
    if concurrency == 0 {
        return Err(PyValueError::new_err("concurrency must be at least 1"));
    }
    Ok(())
}

fn to_array(py: Python<'_>, shape: (usize, usize, usize), data: Vec<u8>) -> PyResult<Bound<'_, PyArray3<u8>>> {
    let array = Array3::from_shape_vec(shape, data).map_err(|e| PyValueError::new_err(e.to_string()))?;
    Ok(array.into_pyarray(py))
}
