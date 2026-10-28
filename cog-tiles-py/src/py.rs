//! The Python classes and functions.

use std::collections::BTreeMap;
use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use cog_tiles::{CogResult, MosaicOptions, PixelWindow, TileCoord, TileEncoding, TilePixels, lnglat_to_mercator};
use numpy::ndarray::Array3;
use numpy::{IntoPyArray as _, PyArray3, PyUntypedArrayMethods as _};
use pyo3::exceptions::{PyStopAsyncIteration, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict};
use pyo3_stub_gen::derive::{gen_stub_pyclass, gen_stub_pyfunction, gen_stub_pymethods};
use tokio::sync::{Mutex, mpsc};
use tokio_util::task::AbortOnDropHandle;

use crate::error::to_py;
use crate::location::{Location, split_mosaic};
use crate::runtime::{runtime, spawn, wait};
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
        let tile = wait(py, self.source.tile(coord))??;
        Ok(tile.map(|data| PyBytes::new(py, &data)))
    }

    /// `tile` for `asyncio`, fetching on background threads instead of the event loop.
    async fn tile_async(&self, z: u8, x: u32, y: u32) -> PyResult<Option<Py<PyBytes>>> {
        let coord = TileCoord::new(z, x, y).map_err(to_py)?;
        let source = Arc::clone(&self.source);
        let tile = spawn(async move { source.tile(coord).await }).await??;
        Ok(tile.map(|data| Python::attach(|py| PyBytes::new(py, &data).unbind())))
    }

    /// The decoded tile `z/x/y` as a `(tile_size, tile_size, bands)` `uint8` array,
    /// or `None` if there is no data there.
    fn tile_pixels<'py>(&self, py: Python<'py>, z: u8, x: u32, y: u32) -> PyResult<Option<Bound<'py, PyArray3<u8>>>> {
        let coord = TileCoord::new(z, x, y).map_err(to_py)?;
        let source = Arc::clone(&self.source);
        let pixels = wait(py, async move { source.pixels(coord).await })?.map_err(to_py)?;
        pixels.map(|pixels| pixels_to_array(py, pixels)).transpose()
    }

    /// `tile_pixels` for `asyncio`, fetching and decoding on background threads instead of the event loop.
    async fn tile_pixels_async(&self, z: u8, x: u32, y: u32) -> PyResult<Option<Py<PyArray3<u8>>>> {
        let coord = TileCoord::new(z, x, y).map_err(to_py)?;
        let source = Arc::clone(&self.source);
        let pixels = spawn(async move { source.pixels(coord).await }).await?.map_err(to_py)?;
        Python::attach(|py| {
            pixels
                .map(|pixels| pixels_to_array(py, pixels).map(Bound::unbind))
                .transpose()
        })
    }

    /// Iterates over the encoded tiles with data in `bbox` at `zoom`, as `(z, x, y, data)`,
    /// fetching `concurrency` at a time in the background in whatever order they arrive.
    /// Use it with `for` or `async for`.
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
            _task: AbortOnDropHandle::new(task),
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
        let window = self.checked_window(bbox, zoom, crs, concurrency, max_pixels)?;
        let raster = wait(py, read_window(Arc::clone(&self.source), window, concurrency))??;
        raster
            .map(|(data, bands)| Raster::new(py, window, data, bands))
            .transpose()
    }

    /// `read` for `asyncio`, fetching and decoding on background threads instead of the event loop.
    #[pyo3(signature = (
        bbox,
        zoom = None,
        *,
        crs = "EPSG:4326",
        concurrency = DEFAULT_CONCURRENCY,
        max_pixels = DEFAULT_MAX_PIXELS,
    ))]
    async fn read_async(
        &self,
        #[gen_stub(override_type(type_repr = "tuple[builtins.float, builtins.float, builtins.float, builtins.float]", imports = ("builtins")))]
        bbox: [f64; 4],
        zoom: Option<u8>,
        #[gen_stub(override_type(type_repr = "typing.Literal['EPSG:4326', 'EPSG:3857']", imports = ("typing")))]
        crs: &str,
        concurrency: usize,
        max_pixels: u64,
    ) -> PyResult<Option<Raster>> {
        let window = self.checked_window(bbox, zoom, crs, concurrency, max_pixels)?;
        let raster = spawn(read_window(Arc::clone(&self.source), window, concurrency)).await??;
        Python::attach(|py| {
            raster
                .map(|(data, bands)| Raster::new(py, window, data, bands))
                .transpose()
        })
    }
}

impl TileSource {
    /// Wraps `source` with the layout it reports.
    fn new(source: Source) -> Self {
        Self {
            layout: source.layout(),
            source: Arc::new(source),
        }
    }

    /// The window `read` stitches, refused if it has more than `max_pixels`.
    fn checked_window(
        &self,
        bbox: [f64; 4],
        zoom: Option<u8>,
        crs: &str,
        concurrency: usize,
        max_pixels: u64,
    ) -> PyResult<PixelWindow> {
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
        Ok(window)
    }
}

/// One tile-aligned Cloud Optimized `GeoTIFF`.
#[gen_stub_pyclass]
#[pyclass(frozen, extends = TileSource, module = "cog_tiles")]
pub struct Cog {
    location: PathBuf,
    options: BTreeMap<String, String>,
    encoding: EncodingArgs,
}

#[gen_stub_pymethods]
#[pymethods]
impl Cog {
    #[gen_stub(skip)]
    fn __reduce__<'py>(slf: &Bound<'py, Self>) -> PyResult<(Bound<'py, PyAny>, CogArgs)> {
        let this = slf.get();
        let open = with_keywords(slf.py(), "open", &this.encoding.keywords(slf.py())?)?;
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

impl Cog {
    /// Opens the COG at `location`, the work that `open` and `open_async` wait for.
    fn open_source(&self) -> PyResult<impl Future<Output = CogResult<Source>> + Send + 'static> {
        let encoding = Some(self.encoding.encoding());
        Ok(Source::open(parse_location(&self.location, &self.options)?, encoding))
    }

    /// The Python object for this COG, read through `source`.
    fn into_py(self, py: Python<'_>, source: Source) -> PyResult<Py<Self>> {
        Py::new(py, PyClassInitializer::from(TileSource::new(source)).add_subclass(self))
    }
}

/// Many COGs, each covering one tile at `file_zoom` and named after it.
#[gen_stub_pyclass]
#[pyclass(frozen, extends = TileSource, module = "cog_tiles")]
pub struct Mosaic {
    location: String,
    options: MosaicOptions,
    store_options: BTreeMap<String, String>,
    encoding: EncodingArgs,
}

/// The arguments `open_mosaic` is called with to restore a pickled mosaic.
type MosaicArgs = (String, u8, u64, f64, Option<f64>, BTreeMap<String, String>);

impl Mosaic {
    /// Checks the arguments of `open_mosaic` and `open_mosaic_async`.
    fn new(
        location: String,
        file_zoom: u8,
        max_cached_files: u64,
        file_expiry: f64,
        file_idle_timeout: Option<f64>,
        options: Option<BTreeMap<String, String>>,
        encoding: EncodingArgs,
    ) -> PyResult<Self> {
        let seconds = |value: f64| {
            Duration::try_from_secs_f64(value)
                .map_err(|e| PyValueError::new_err(format!("{value} is not a valid number of seconds: {e}")))
        };
        let mosaic_options = MosaicOptions::new(file_zoom)
            .with_max_cached_files(max_cached_files)
            .with_file_expiry(seconds(file_expiry)?)
            .with_file_idle_timeout(file_idle_timeout.map(seconds).transpose()?);
        Ok(Self {
            location,
            options: mosaic_options,
            store_options: options.unwrap_or_default(),
            encoding,
        })
    }

    /// Lists the location for one file, the work that `open_mosaic` and `open_mosaic_async` wait for.
    fn open_source(&self) -> PyResult<impl Future<Output = CogResult<Source>> + Send + 'static> {
        let (root, template) = split_mosaic(&self.location);
        let pairs: Vec<_> = self.store_options.clone().into_iter().collect();
        let root = Location::parse(&root, &pairs)?;
        let template = template.to_owned();
        let options = self.options;
        let encoding = Some(self.encoding.encoding());
        Ok(async move { Source::open_mosaic(root, &template, options, encoding).await })
    }

    /// The Python object for this mosaic, read through `source`.
    fn into_py(self, py: Python<'_>, source: Source) -> PyResult<Py<Self>> {
        Py::new(py, PyClassInitializer::from(TileSource::new(source)).add_subclass(self))
    }
}

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
        let open = with_keywords(slf.py(), "open_mosaic", &this.encoding.keywords(slf.py())?)?;
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

impl Raster {
    /// The raster of `window` from its stitched pixels.
    fn new(py: Python<'_>, window: PixelWindow, data: Vec<u8>, bands: usize) -> PyResult<Self> {
        let shape = (window.height() as usize, window.width() as usize, bands);
        Ok(Self {
            data: to_array(py, shape, data)?.unbind(),
            transform: window.geotransform(),
            bounds: window.mercator_bounds(),
            zoom: window.zoom(),
        })
    }
}

/// One tile from a [`TileIterator`]: `(z, x, y, data)`.
type TileItem = (u8, u32, u32, Py<PyBytes>);

/// Encoded tiles as `(z, x, y, data)`, fetched in the background, for `for` and `async for`.
/// Dropping it stops the fetches.
#[gen_stub_pyclass]
#[pyclass(frozen, module = "cog_tiles")]
pub struct TileIterator {
    receiver: Mutex<mpsc::Receiver<PyResult<(TileCoord, bytes::Bytes)>>>,
    _task: AbortOnDropHandle<()>,
}

#[gen_stub_pymethods]
#[pymethods]
impl TileIterator {
    fn __iter__(slf: Py<Self>) -> Py<Self> {
        slf
    }

    #[gen_stub(override_return_type(type_repr = "tuple[builtins.int, builtins.int, builtins.int, builtins.bytes]", imports = ("builtins")))]
    fn __next__(&self, py: Python<'_>) -> PyResult<Option<TileItem>> {
        let next = wait(py, async { self.receiver.lock().await.recv().await })?;
        next.map(|item| to_tile_item(py, item)).transpose()
    }

    fn __aiter__(slf: Py<Self>) -> Py<Self> {
        slf
    }

    #[gen_stub(override_return_type(type_repr = "typing.Awaitable[tuple[builtins.int, builtins.int, builtins.int, builtins.bytes]]", imports = ("builtins", "typing")))]
    fn __anext__<'py>(slf: &Bound<'py, Self>) -> PyResult<Bound<'py, PyAny>> {
        slf.call_method0(pyo3::intern!(slf.py(), "_anext"))
    }

    /// The coroutine `__anext__` returns, since `pyo3` cannot make `__anext__` itself `async`.
    #[gen_stub(skip)]
    async fn _anext(&self) -> PyResult<TileItem> {
        let next = self.receiver.lock().await.recv().await;
        let item = next.ok_or_else(|| PyStopAsyncIteration::new_err(()))?;
        Python::attach(|py| to_tile_item(py, item))
    }
}

/// Opens one COG from a local path or a URL (`s3://`, `gs://`, `az://`, `https://`, `file://`).
///
/// Object store credentials come from the usual environment variables (e.g. `AWS_REGION`,
/// `AWS_ACCESS_KEY_ID`), overridden by `options` (e.g. `{"aws_skip_signature": "true"}`).
///
/// `format` (`"png"`, `"webp"` or `"jpeg"`) re-encodes every tile, with `quality` from 1 to 100
/// (80 when unset) or `"lossless"` for WebP, and JPEG painted over `background`, a CSS color or
/// `(red, green, blue)` (white when unset). `format` is `"webp"` when unset.
#[gen_stub_pyfunction]
#[pyfunction]
#[pyo3(signature = (location, options = None, *, format = None, quality = None, background = None))]
pub fn open(
    py: Python<'_>,
    location: PathBuf,
    options: Option<BTreeMap<String, String>>,
    #[gen_stub(override_type(type_repr = "typing.Literal['png', 'webp', 'jpeg'] | None", imports = ("typing")))]
    format: Option<Format>,
    #[gen_stub(override_type(type_repr = "builtins.int | typing.Literal['lossless'] | None", imports = ("builtins", "typing")))]
    quality: Option<Quality>,
    #[gen_stub(override_type(type_repr = "builtins.str | tuple[builtins.int, builtins.int, builtins.int] | None", imports = ("builtins")))]
    background: Option<Background>,
) -> PyResult<Py<Cog>> {
    let cog = Cog {
        location,
        options: options.unwrap_or_default(),
        encoding: EncodingArgs::new(format, quality, background)?,
    };
    let source = wait(py, cog.open_source()?)?.map_err(to_py)?;
    cog.into_py(py, source)
}

/// `open` for `asyncio`, reading the headers on background threads instead of the event loop.
#[gen_stub_pyfunction]
#[pyfunction]
#[pyo3(signature = (location, options = None, *, format = None, quality = None, background = None))]
pub async fn open_async(
    location: PathBuf,
    options: Option<BTreeMap<String, String>>,
    #[gen_stub(override_type(type_repr = "typing.Literal['png', 'webp', 'jpeg'] | None", imports = ("typing")))]
    format: Option<Format>,
    #[gen_stub(override_type(type_repr = "builtins.int | typing.Literal['lossless'] | None", imports = ("builtins", "typing")))]
    quality: Option<Quality>,
    #[gen_stub(override_type(type_repr = "builtins.str | tuple[builtins.int, builtins.int, builtins.int] | None", imports = ("builtins")))]
    background: Option<Background>,
) -> PyResult<Py<Cog>> {
    let cog = Cog {
        location,
        options: options.unwrap_or_default(),
        encoding: EncodingArgs::new(format, quality, background)?,
    };
    let source = spawn(cog.open_source()?).await?.map_err(to_py)?;
    Python::attach(|py| cog.into_py(py, source))
}

/// Opens a mosaic of COGs that each cover one tile at `file_zoom`, named by a template in
/// `location`: `{quadkey}`, or `{x}` and `{y}` with an optional `{z}`, e.g.
/// `"s3://bucket/ortho/{quadkey}.tif"` or `"/data/ortho/{z}/{x}/{y}.tif"`.
/// `{-y}` is the row counted from the bottom (TMS).
///
/// One file is found by listing the location now; the others are opened on first use and kept
/// for `file_expiry` seconds (or `file_idle_timeout` seconds unused), at most `max_cached_files`.
/// Square files without `GeoTIFF` tags are placed on the tile in their file name.
///
/// `format`, `quality` and `background` re-encode every tile, as in `open`.
#[gen_stub_pyfunction]
#[pyfunction]
#[pyo3(signature = (
    location,
    file_zoom,
    max_cached_files = cog_tiles::DEFAULT_MAX_CACHED_FILES,
    file_expiry = cog_tiles::DEFAULT_FILE_EXPIRY.as_secs_f64(),
    file_idle_timeout = None,
    options = None,
    *,
    format = None,
    quality = None,
    background = None,
))]
#[expect(clippy::too_many_arguments, reason = "the arguments of a Python function")]
pub fn open_mosaic(
    py: Python<'_>,
    location: String,
    file_zoom: u8,
    max_cached_files: u64,
    file_expiry: f64,
    file_idle_timeout: Option<f64>,
    options: Option<BTreeMap<String, String>>,
    #[gen_stub(override_type(type_repr = "typing.Literal['png', 'webp', 'jpeg'] | None", imports = ("typing")))]
    format: Option<Format>,
    #[gen_stub(override_type(type_repr = "builtins.int | typing.Literal['lossless'] | None", imports = ("builtins", "typing")))]
    quality: Option<Quality>,
    #[gen_stub(override_type(type_repr = "builtins.str | tuple[builtins.int, builtins.int, builtins.int] | None", imports = ("builtins")))]
    background: Option<Background>,
) -> PyResult<Py<Mosaic>> {
    let mosaic = Mosaic::new(
        location,
        file_zoom,
        max_cached_files,
        file_expiry,
        file_idle_timeout,
        options,
        EncodingArgs::new(format, quality, background)?,
    )?;
    let source = wait(py, mosaic.open_source()?)?.map_err(to_py)?;
    mosaic.into_py(py, source)
}

/// `open_mosaic` for `asyncio`, listing and reading on background threads instead of the event loop.
#[gen_stub_pyfunction]
#[pyfunction]
#[pyo3(signature = (
    location,
    file_zoom,
    max_cached_files = cog_tiles::DEFAULT_MAX_CACHED_FILES,
    file_expiry = cog_tiles::DEFAULT_FILE_EXPIRY.as_secs_f64(),
    file_idle_timeout = None,
    options = None,
    *,
    format = None,
    quality = None,
    background = None,
))]
#[expect(clippy::too_many_arguments, reason = "the arguments of a Python function")]
pub async fn open_mosaic_async(
    location: String,
    file_zoom: u8,
    max_cached_files: u64,
    file_expiry: f64,
    file_idle_timeout: Option<f64>,
    options: Option<BTreeMap<String, String>>,
    #[gen_stub(override_type(type_repr = "typing.Literal['png', 'webp', 'jpeg'] | None", imports = ("typing")))]
    format: Option<Format>,
    #[gen_stub(override_type(type_repr = "builtins.int | typing.Literal['lossless'] | None", imports = ("builtins", "typing")))]
    quality: Option<Quality>,
    #[gen_stub(override_type(type_repr = "builtins.str | tuple[builtins.int, builtins.int, builtins.int] | None", imports = ("builtins")))]
    background: Option<Background>,
) -> PyResult<Py<Mosaic>> {
    let mosaic = Mosaic::new(
        location,
        file_zoom,
        max_cached_files,
        file_expiry,
        file_idle_timeout,
        options,
        EncodingArgs::new(format, quality, background)?,
    )?;
    let source = spawn(mosaic.open_source()?).await?.map_err(to_py)?;
    Python::attach(|py| mosaic.into_py(py, source))
}

/// `cog_tiles.<function>` with `keywords` bound, for `__reduce__` to restore keyword-only arguments.
fn with_keywords<'py>(py: Python<'py>, function: &str, keywords: &Bound<'py, PyDict>) -> PyResult<Bound<'py, PyAny>> {
    let function = py.import("cog_tiles")?.getattr(function)?;
    py.import("functools")?
        .getattr("partial")?
        .call((function,), Some(keywords))
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

/// A lossy quality from 1 (smallest) to 100 (best), or lossless.
#[derive(Clone, Copy)]
pub enum Quality {
    /// No loss, for WebP only.
    Lossless,
    /// A lossy quality from 1 to 100.
    Level(u8),
}

impl<'py> FromPyObject<'_, 'py> for Quality {
    type Error = PyErr;

    fn extract(value: Borrowed<'_, 'py, PyAny>) -> PyResult<Self> {
        if let Ok(name) = value.extract::<String>() {
            return match name.as_str() {
                "lossless" => Ok(Self::Lossless),
                _ => Err(PyValueError::new_err(format!(
                    "quality must be 1 to 100 or \"lossless\", not {name:?}"
                ))),
            };
        }
        let level: i64 = value.extract()?;
        match u8::try_from(level) {
            Ok(level @ 1..=100) => Ok(Self::Level(level)),
            _ => Err(PyValueError::new_err(format!("quality must be 1 to 100, not {level}"))),
        }
    }
}

impl<'py> IntoPyObject<'py> for Quality {
    type Target = PyAny;
    type Output = Bound<'py, PyAny>;
    type Error = PyErr;

    fn into_pyobject(self, py: Python<'py>) -> PyResult<Self::Output> {
        match self {
            Self::Lossless => Ok("lossless".into_pyobject(py)?.into_any()),
            Self::Level(level) => Ok(level.into_pyobject(py)?.into_any()),
        }
    }
}

/// An opaque color, given as a CSS string such as `"white"` or `"#336699"`, or as `(red, green, blue)`.
#[derive(Clone, Copy, IntoPyObject)]
pub struct Background((u8, u8, u8));

impl<'py> FromPyObject<'_, 'py> for Background {
    type Error = PyErr;

    fn extract(value: Borrowed<'_, 'py, PyAny>) -> PyResult<Self> {
        if let Ok(css) = value.extract::<String>() {
            return match csscolorparser::parse(&css)
                .map_err(|e| PyValueError::new_err(format!("background {css:?}: {e}")))?
                .to_rgba8()
            {
                [r, g, b, 255] => Ok(Self((r, g, b))),
                _ => Err(PyValueError::new_err(format!(
                    "background must be an opaque CSS color, not {css:?}"
                ))),
            };
        }
        Ok(Self(value.extract()?))
    }
}

/// The tile format a source re-encodes to.
#[derive(Clone, Copy, Default, PartialEq, Eq, strum::Display, strum::EnumString, strum::VariantNames)]
#[strum(serialize_all = "lowercase")]
pub enum Format {
    /// Lossless PNG.
    Png,
    /// WebP, lossy or lossless, the default.
    #[default]
    Webp,
    /// Lossy JPEG, painted over a background where tiles are transparent.
    Jpeg,
}

impl<'py> FromPyObject<'_, 'py> for Format {
    type Error = PyErr;

    fn extract(value: Borrowed<'_, 'py, PyAny>) -> PyResult<Self> {
        let name: String = value.extract()?;
        name.parse().map_err(|_unknown| {
            PyValueError::new_err(format!("format must be \"png\", \"webp\" or \"jpeg\", not {name:?}"))
        })
    }
}

/// The `format`, `quality` and `background` a source re-encodes its tiles with.
#[derive(Clone)]
struct EncodingArgs {
    format: Option<Format>,
    quality: Option<Quality>,
    background: Option<Background>,
    encoding: TileEncoding,
}

impl EncodingArgs {
    /// The quality when `quality` is unset, as in Martin.
    const DEFAULT_QUALITY: u8 = 80;

    /// The color JPEG tiles show where they are transparent when `background` is unset, as in Martin.
    const DEFAULT_BACKGROUND: (u8, u8, u8) = (255, 255, 255);

    /// The arguments as given to `open` or `open_mosaic`, rejecting a `quality` or `background` the `format` cannot use.
    fn new(format: Option<Format>, quality: Option<Quality>, background: Option<Background>) -> PyResult<Self> {
        let used = format.unwrap_or_default();
        if quality.is_some() && !matches!(used, Format::Webp | Format::Jpeg) {
            return Err(PyValueError::new_err("quality needs format webp or jpeg"));
        }
        if background.is_some() && used != Format::Jpeg {
            return Err(PyValueError::new_err("background needs format jpeg"));
        }
        let level = |quality| match quality {
            Some(Quality::Level(level)) => level,
            _ => Self::DEFAULT_QUALITY,
        };
        let encoding = match used {
            Format::Png => TileEncoding::Png,
            Format::Webp => TileEncoding::Webp {
                quality: match quality {
                    Some(Quality::Lossless) => None,
                    quality => Some(level(quality)),
                },
            },
            Format::Jpeg if matches!(quality, Some(Quality::Lossless)) => {
                return Err(PyValueError::new_err(
                    "JPEG has no lossless mode, so quality must be 1 to 100",
                ));
            }
            Format::Jpeg => {
                let (r, g, b) = background.map_or(Self::DEFAULT_BACKGROUND, |Background(rgb)| rgb);
                TileEncoding::Jpeg {
                    quality: level(quality),
                    background: [r, g, b],
                }
            }
        };
        Ok(Self {
            format,
            quality,
            background,
            encoding,
        })
    }

    /// The encoding these arguments ask for.
    fn encoding(&self) -> TileEncoding {
        self.encoding
    }

    /// These arguments as the keywords of `open`.
    fn keywords<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let keywords = PyDict::new(py);
        keywords.set_item("format", self.format.map(|f| f.to_string()))?;
        keywords.set_item("quality", self.quality)?;
        keywords.set_item("background", self.background)?;
        Ok(keywords)
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

/// The decoded tile as a `(size, size, bands)` array, without copying.
fn pixels_to_array(py: Python<'_>, pixels: TilePixels) -> PyResult<Bound<'_, PyArray3<u8>>> {
    let size = pixels.size() as usize;
    let shape = (size, size, pixels.bands());
    to_array(py, shape, pixels.into_data())
}

/// One received tile as the `(z, x, y, data)` a `TileIterator` yields.
fn to_tile_item(py: Python<'_>, item: PyResult<(TileCoord, bytes::Bytes)>) -> PyResult<TileItem> {
    let (coord, data) = item?;
    Ok((coord.z(), coord.x(), coord.y(), PyBytes::new(py, &data).unbind()))
}

fn to_array(py: Python<'_>, shape: (usize, usize, usize), data: Vec<u8>) -> PyResult<Bound<'_, PyArray3<u8>>> {
    let array = Array3::from_shape_vec(shape, data).map_err(|e| PyValueError::new_err(e.to_string()))?;
    Ok(array.into_pyarray(py))
}
