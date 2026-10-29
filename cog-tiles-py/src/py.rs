//! The Python classes and functions.

use std::collections::BTreeMap;
use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use cog_tiles::{
    AsyncIndexedMosaic, CogResult, IndexedMosaicOptions, MosaicOptions, PixelWindow, StacIndex, StacOrder,
    StacSearchOptions, TileCoord, TileEncoding, TilePixels, lnglat_to_mercator,
};
use numpy::ndarray::Array3;
use numpy::{IntoPyArray as _, PyArray3, PyUntypedArrayMethods as _};
use pyo3::exceptions::{PyStopAsyncIteration, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict, PyMapping, PyString};
use pyo3_stub_gen::derive::{gen_stub_pyclass, gen_stub_pyfunction, gen_stub_pymethods};
use pyo3_stub_gen::{PyStubType, TypeInfo};
use tokio::sync::{Mutex, mpsc};
use tokio_util::task::AbortOnDropHandle;

use crate::error::to_py;
use crate::location::{Location, href_resolver, split_mosaic};
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

    /// `(west, south, east, north)` in degrees, or `None` for a `Mosaic`.
    #[getter]
    fn bounds(&self) -> Option<(f64, f64, f64, f64)> {
        self.source.bounds().map(|(bounds, _)| bounds.into())
    }

    /// `(min_x, min_y, max_x, max_y)` in EPSG:3857 meters, or `None` for a `Mosaic`.
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
    /// `bbox` is the source's bounds when `None`; a `Mosaic` needs one.
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
        let encoding = self.encoding.encoding();
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
        let encoding = self.encoding.encoding();
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

/// The COGs a `gdaltindex` tile index lists, in any supported projection, reprojected onto Web Mercator.
#[gen_stub_pyclass]
#[pyclass(frozen, extends = TileSource, module = "cog_tiles")]
pub struct TileIndex {
    path: PathBuf,
    min_zoom: u8,
    layer: Option<String>,
    location_field: String,
    bands: Option<Vec<usize>>,
    index: IndexArgs,
    store_options: BTreeMap<String, String>,
    encoding: EncodingArgs,
}

impl TileIndex {
    /// Reads the index and opens its first file, the work that `open_tile_index` and
    /// `open_tile_index_async` wait for.
    fn open_source(&self) -> PyResult<impl Future<Output = CogResult<Source>> + Send + 'static> {
        let options = self
            .index
            .options(self.min_zoom, from_one(self.bands.as_deref().unwrap_or(&[1, 2, 3]))?)?;
        let encoding = self.encoding.encoding();
        let resolve = href_resolver(self.store_options.clone().into_iter().collect());
        let path = self.path.clone();
        let layer = self.layer.clone();
        let location_field = self.location_field.clone();
        Ok(async move {
            let index = cog_tiles::TileIndex::open(&path, layer.as_deref(), &location_field, &resolve).await?;
            let mosaic = AsyncIndexedMosaic::open_tile_index(index, options).await?;
            Ok(Source::indexed(mosaic, encoding))
        })
    }

    /// The Python object for this tile index, read through `source`.
    fn into_py(self, py: Python<'_>, source: Source) -> PyResult<Py<Self>> {
        Py::new(py, PyClassInitializer::from(TileSource::new(source)).add_subclass(self))
    }
}

#[gen_stub_pymethods]
#[pymethods]
impl TileIndex {
    #[gen_stub(skip)]
    fn __reduce__<'py>(slf: &Bound<'py, Self>) -> PyResult<(Bound<'py, PyAny>, (PathBuf, u8))> {
        let this = slf.get();
        let keywords = this.encoding.keywords(slf.py())?;
        this.index.add_keywords(&keywords)?;
        keywords.set_item("layer", &this.layer)?;
        keywords.set_item("location_field", &this.location_field)?;
        keywords.set_item("bands", &this.bands)?;
        keywords.set_item("options", &this.store_options)?;
        let open = with_keywords(slf.py(), "open_tile_index", &keywords)?;
        Ok((open, (this.path.clone(), this.min_zoom)))
    }

    fn __repr__(slf: &Bound<'_, Self>) -> String {
        let layout = slf.as_super().get().layout;
        format!(
            "TileIndex({:?}, zooms={}..={}, format={:?})",
            slf.get().path.display().to_string(),
            layout.min_zoom,
            layout.max_zoom,
            layout.format.extension(),
        )
    }
}

/// The COGs a STAC API finds for each tile, in any supported projection, reprojected onto Web Mercator.
#[gen_stub_pyclass]
#[pyclass(frozen, extends = TileSource, module = "cog_tiles")]
pub struct Stac {
    /// The STAC API root.
    url: String,
    /// The collection searched.
    collection: String,
    /// The key of the asset read from each item.
    asset: String,
    /// The lowest zoom served.
    min_zoom: u8,
    /// How the STAC API is searched.
    search: SearchArgs,
    /// The selected bands, red, green and blue when `None`.
    bands: Option<Vec<Band>>,
    /// The mosaic options shared with `open_tile_index`.
    index: IndexArgs,
    /// The object store options assets are read with.
    store_options: BTreeMap<String, String>,
    /// How tiles are re-encoded.
    encoding: EncodingArgs,
}

/// The positional arguments `open_stac` is called with to restore a pickled STAC source.
type StacArgs = (String, String, String, u8);

/// How a STAC API is searched.
#[derive(Clone)]
struct SearchArgs {
    /// The zoom one search covers a tile of, `min_zoom` when `None`.
    search_zoom: Option<u8>,
    /// The STAC `datetime` interval items must fall in.
    datetime: Option<String>,
    /// The STAC query as JSON text.
    query: Option<String>,
    /// Which items are painted first.
    order: Order,
    /// The most items one search reads.
    max_items_per_search: usize,
    /// Seconds a search result is kept.
    search_expiry: f64,
    /// The most search results kept at once.
    max_cached_searches: u64,
}

/// One band of a STAC asset, by number (counted from 1) or by `eo:bands` name.
#[derive(Clone, FromPyObject, IntoPyObject)]
pub enum Band {
    /// The band number, counted from 1.
    Number(usize),
    /// The band's `eo:bands` name.
    Name(String),
}

impl Stac {
    /// Reads the collection and opens its first matching item, the work that `open_stac` and
    /// `open_stac_async` wait for.
    fn open_source(&self) -> PyResult<impl Future<Output = CogResult<Source>> + Send + 'static> {
        let order = match self.search.order {
            Order::Newest => StacOrder::Newest,
            Order::Oldest => StacOrder::Oldest,
        };
        let query = self
            .search
            .query
            .as_deref()
            .map(serde_json::from_str)
            .transpose()
            .map_err(|e| PyValueError::new_err(format!("query: {e}")))?;
        let search = StacSearchOptions {
            url: self.url.clone(),
            user_agent: concat!("cog-tiles-py/", env!("CARGO_PKG_VERSION")).to_owned(),
            collection: self.collection.clone(),
            asset: self.asset.clone(),
            datetime: self.search.datetime.clone(),
            query,
            order,
            search_zoom: self.search.search_zoom.unwrap_or(self.min_zoom),
            max_items_per_search: self.search.max_items_per_search,
            search_expiry: seconds(self.search.search_expiry)?,
            max_cached_searches: self.search.max_cached_searches,
        };
        let bands: Option<Vec<cog_tiles::StacBand>> = self.bands.as_ref().map(|bands| {
            bands
                .iter()
                .map(|band| match band {
                    Band::Number(number) => cog_tiles::StacBand::Number(*number),
                    Band::Name(name) => cog_tiles::StacBand::Name(name.clone()),
                })
                .collect()
        });
        let mut options = self.index.options(self.min_zoom, Vec::new())?;
        let encoding = self.encoding.encoding();
        let resolve = href_resolver(self.store_options.clone().into_iter().collect());
        Ok(async move {
            let index = StacIndex::open(search, bands.as_deref(), resolve).await?;
            options.bands = index.bands().to_vec();
            let mosaic = AsyncIndexedMosaic::open_stac(index, options).await?;
            Ok(Source::indexed(mosaic, encoding))
        })
    }

    /// The Python object for this STAC source, read through `source`.
    fn into_py(self, py: Python<'_>, source: Source) -> PyResult<Py<Self>> {
        Py::new(py, PyClassInitializer::from(TileSource::new(source)).add_subclass(self))
    }
}

#[gen_stub_pymethods]
#[pymethods]
impl Stac {
    #[gen_stub(skip)]
    fn __reduce__<'py>(slf: &Bound<'py, Self>) -> PyResult<(Bound<'py, PyAny>, StacArgs)> {
        let this = slf.get();
        let py = slf.py();
        let keywords = this.encoding.keywords(py)?;
        this.index.add_keywords(&keywords)?;
        let query = match &this.search.query {
            Some(query) => Some(py.import("json")?.call_method1("loads", (query,))?),
            None => None,
        };
        keywords.set_item("search_zoom", this.search.search_zoom)?;
        keywords.set_item("datetime", &this.search.datetime)?;
        keywords.set_item("query", query)?;
        keywords.set_item("order", this.search.order)?;
        keywords.set_item("max_items_per_search", this.search.max_items_per_search)?;
        keywords.set_item("search_expiry", this.search.search_expiry)?;
        keywords.set_item("max_cached_searches", this.search.max_cached_searches)?;
        keywords.set_item("bands", this.bands.clone())?;
        keywords.set_item("options", &this.store_options)?;
        let open = with_keywords(py, "open_stac", &keywords)?;
        Ok((
            open,
            (
                this.url.clone(),
                this.collection.clone(),
                this.asset.clone(),
                this.min_zoom,
            ),
        ))
    }

    fn __repr__(slf: &Bound<'_, Self>) -> String {
        let layout = slf.as_super().get().layout;
        let this = slf.get();
        format!(
            "Stac({:?}, collection={:?}, asset={:?}, zooms={}..={}, format={:?})",
            this.url,
            this.collection,
            this.asset,
            layout.min_zoom,
            layout.max_zoom,
            layout.format.extension(),
        )
    }
}

/// The zooms, nodata and file cache of a mosaic found by an index.
#[derive(Clone)]
struct IndexArgs {
    max_zoom: Option<u8>,
    nodata: Option<u8>,
    max_cached_files: u64,
    file_expiry: f64,
    file_idle_timeout: Option<f64>,
}

impl IndexArgs {
    /// The options of a mosaic from `min_zoom` serving `bands`, counted from 0.
    fn options(&self, min_zoom: u8, bands: Vec<usize>) -> PyResult<IndexedMosaicOptions> {
        Ok(IndexedMosaicOptions::new(min_zoom, bands)
            .with_max_zoom(self.max_zoom)
            .with_nodata(self.nodata)
            .with_max_cached_files(self.max_cached_files)
            .with_file_expiry(seconds(self.file_expiry)?)
            .with_file_idle_timeout(self.file_idle_timeout.map(seconds).transpose()?))
    }

    /// Adds these arguments to `keywords`.
    fn add_keywords(&self, keywords: &Bound<'_, PyDict>) -> PyResult<()> {
        keywords.set_item("max_zoom", self.max_zoom)?;
        keywords.set_item("nodata", self.nodata)?;
        keywords.set_item("max_cached_files", self.max_cached_files)?;
        keywords.set_item("file_expiry", self.file_expiry)?;
        keywords.set_item("file_idle_timeout", self.file_idle_timeout)
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
/// `(red, green, blue)` (white when unset). Without it, JPEG and WebP tiles are returned as stored
/// and others as PNG.
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

/// Opens the COGs a `gdaltindex` tile index lists: a `GeoPackage` with one footprint and location
/// per file, e.g. `gdaltindex -f GPKG -t_srs EPSG:4326 index.gpkg *.tif`.
///
/// The files may be in Web Mercator or a UTM zone, tiled with overviews down to `min_zoom`; each
/// tile is painted from the files in the index's row order, reprojected with nearest-neighbour
/// resampling, as PNG with an alpha band. `layer` is the feature table to read, the only one when
/// `None`. `bands` are the bands served, counted from 1: one for gray, three for RGB.
/// `nodata` overrides each file's nodata value. `max_zoom` is the zoom at which the first file is
/// fully detailed when `None`.
///
/// Locations may be local paths (relative to the index), URLs, or GDAL `/vsis3/`, `/vsigs/`,
/// `/vsiaz/` or `/vsicurl/` paths, read with `options` as in `open`. The whole index is read now,
/// and its first file is opened. `format`, `quality` and `background` re-encode every tile, as in `open`.
#[gen_stub_pyfunction]
#[pyfunction]
#[pyo3(signature = (
    path,
    min_zoom,
    *,
    layer = None,
    location_field = cog_tiles::DEFAULT_LOCATION_FIELD.to_owned(),
    max_zoom = None,
    bands = None,
    nodata = None,
    max_cached_files = cog_tiles::DEFAULT_MAX_CACHED_FILES,
    file_expiry = cog_tiles::DEFAULT_FILE_EXPIRY.as_secs_f64(),
    file_idle_timeout = None,
    options = None,
    format = None,
    quality = None,
    background = None,
))]
#[expect(clippy::too_many_arguments, reason = "the arguments of a Python function")]
pub fn open_tile_index(
    py: Python<'_>,
    path: PathBuf,
    min_zoom: u8,
    layer: Option<String>,
    location_field: String,
    max_zoom: Option<u8>,
    bands: Option<Vec<usize>>,
    nodata: Option<u8>,
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
) -> PyResult<Py<TileIndex>> {
    let index = TileIndex {
        path,
        min_zoom,
        layer,
        location_field,
        bands,
        index: IndexArgs {
            max_zoom,
            nodata,
            max_cached_files,
            file_expiry,
            file_idle_timeout,
        },
        store_options: options.unwrap_or_default(),
        encoding: EncodingArgs::new(format, quality, background)?,
    };
    let source = wait(py, index.open_source()?)?.map_err(to_py)?;
    index.into_py(py, source)
}

/// `open_tile_index` for `asyncio`, reading the index and the first file on background threads
/// instead of the event loop.
#[gen_stub_pyfunction]
#[pyfunction]
#[pyo3(signature = (
    path,
    min_zoom,
    *,
    layer = None,
    location_field = cog_tiles::DEFAULT_LOCATION_FIELD.to_owned(),
    max_zoom = None,
    bands = None,
    nodata = None,
    max_cached_files = cog_tiles::DEFAULT_MAX_CACHED_FILES,
    file_expiry = cog_tiles::DEFAULT_FILE_EXPIRY.as_secs_f64(),
    file_idle_timeout = None,
    options = None,
    format = None,
    quality = None,
    background = None,
))]
#[expect(clippy::too_many_arguments, reason = "the arguments of a Python function")]
pub async fn open_tile_index_async(
    path: PathBuf,
    min_zoom: u8,
    layer: Option<String>,
    location_field: String,
    max_zoom: Option<u8>,
    bands: Option<Vec<usize>>,
    nodata: Option<u8>,
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
) -> PyResult<Py<TileIndex>> {
    let index = TileIndex {
        path,
        min_zoom,
        layer,
        location_field,
        bands,
        index: IndexArgs {
            max_zoom,
            nodata,
            max_cached_files,
            file_expiry,
            file_idle_timeout,
        },
        store_options: options.unwrap_or_default(),
        encoding: EncodingArgs::new(format, quality, background)?,
    };
    let source = spawn(index.open_source()?).await?.map_err(to_py)?;
    Python::attach(|py| index.into_py(py, source))
}

/// Opens the COGs a STAC API finds for each tile: the `asset` of the items in `collection`.
///
/// One search covers one tile at `search_zoom` (`min_zoom` when `None`) and serves every tile below
/// it; at most `max_items_per_search` items are read, and results are kept for `search_expiry`
/// seconds, at most `max_cached_searches`. `datetime` is a STAC interval, e.g.
/// `"2020-01-01T00:00:00Z/.."`, and `query` a STAC query extension object, e.g.
/// `{"eo:cloud_cover": {"lt": 10}}`. `order` paints the `"newest"` or `"oldest"` items first.
///
/// `bands` are the bands served, by number (counted from 1) or `eo:bands` name: one for gray, three
/// for RGB; `["red", "green", "blue"]` when `None`, or the first three if the asset does not name its
/// bands. The rest is as in `open_tile_index`: assets are read with `options`, and `format`,
/// `quality` and `background` re-encode every tile.
///
/// The collection is read now for its extent and band names, and its first matching item is opened.
#[gen_stub_pyfunction]
#[pyfunction]
#[pyo3(signature = (
    url,
    collection,
    asset,
    min_zoom,
    *,
    search_zoom = None,
    datetime = None,
    query = None,
    order = Order::Newest,
    max_items_per_search = cog_tiles::DEFAULT_MAX_ITEMS_PER_SEARCH,
    search_expiry = cog_tiles::DEFAULT_SEARCH_EXPIRY.as_secs_f64(),
    max_cached_searches = cog_tiles::DEFAULT_MAX_CACHED_SEARCHES,
    max_zoom = None,
    bands = None,
    nodata = None,
    max_cached_files = cog_tiles::DEFAULT_MAX_CACHED_FILES,
    file_expiry = cog_tiles::DEFAULT_FILE_EXPIRY.as_secs_f64(),
    file_idle_timeout = None,
    options = None,
    format = None,
    quality = None,
    background = None,
))]
#[expect(clippy::too_many_arguments, reason = "the arguments of a Python function")]
pub fn open_stac(
    py: Python<'_>,
    url: String,
    collection: String,
    asset: String,
    min_zoom: u8,
    search_zoom: Option<u8>,
    datetime: Option<String>,
    #[gen_stub(override_type(type_repr = "typing.Mapping[builtins.str, typing.Any] | None", imports = ("builtins", "typing")))]
    query: Option<Py<PyAny>>,
    order: Order,
    max_items_per_search: usize,
    search_expiry: f64,
    max_cached_searches: u64,
    max_zoom: Option<u8>,
    #[gen_stub(override_type(type_repr = "typing.Sequence[builtins.int | builtins.str] | None", imports = ("builtins", "typing")))]
    bands: Option<Vec<Band>>,
    nodata: Option<u8>,
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
) -> PyResult<Py<Stac>> {
    let query = query.map(|query| to_json(&query)).transpose()?;
    let stac = Stac {
        url,
        collection,
        asset,
        min_zoom,
        search: SearchArgs {
            search_zoom,
            datetime,
            query,
            order,
            max_items_per_search,
            search_expiry,
            max_cached_searches,
        },
        bands,
        index: IndexArgs {
            max_zoom,
            nodata,
            max_cached_files,
            file_expiry,
            file_idle_timeout,
        },
        store_options: options.unwrap_or_default(),
        encoding: EncodingArgs::new(format, quality, background)?,
    };
    let source = wait(py, stac.open_source()?)?.map_err(to_py)?;
    stac.into_py(py, source)
}

/// `open_stac` for `asyncio`, reading the API and the first item on background threads instead of
/// the event loop.
#[gen_stub_pyfunction]
#[pyfunction]
#[pyo3(signature = (
    url,
    collection,
    asset,
    min_zoom,
    *,
    search_zoom = None,
    datetime = None,
    query = None,
    order = Order::Newest,
    max_items_per_search = cog_tiles::DEFAULT_MAX_ITEMS_PER_SEARCH,
    search_expiry = cog_tiles::DEFAULT_SEARCH_EXPIRY.as_secs_f64(),
    max_cached_searches = cog_tiles::DEFAULT_MAX_CACHED_SEARCHES,
    max_zoom = None,
    bands = None,
    nodata = None,
    max_cached_files = cog_tiles::DEFAULT_MAX_CACHED_FILES,
    file_expiry = cog_tiles::DEFAULT_FILE_EXPIRY.as_secs_f64(),
    file_idle_timeout = None,
    options = None,
    format = None,
    quality = None,
    background = None,
))]
#[expect(clippy::too_many_arguments, reason = "the arguments of a Python function")]
pub async fn open_stac_async(
    url: String,
    collection: String,
    asset: String,
    min_zoom: u8,
    search_zoom: Option<u8>,
    datetime: Option<String>,
    #[gen_stub(override_type(type_repr = "typing.Mapping[builtins.str, typing.Any] | None", imports = ("builtins", "typing")))]
    query: Option<Py<PyAny>>,
    order: Order,
    max_items_per_search: usize,
    search_expiry: f64,
    max_cached_searches: u64,
    max_zoom: Option<u8>,
    #[gen_stub(override_type(type_repr = "typing.Sequence[builtins.int | builtins.str] | None", imports = ("builtins", "typing")))]
    bands: Option<Vec<Band>>,
    nodata: Option<u8>,
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
) -> PyResult<Py<Stac>> {
    let query = query.map(|query| to_json(&query)).transpose()?;
    let stac = Stac {
        url,
        collection,
        asset,
        min_zoom,
        search: SearchArgs {
            search_zoom,
            datetime,
            query,
            order,
            max_items_per_search,
            search_expiry,
            max_cached_searches,
        },
        bands,
        index: IndexArgs {
            max_zoom,
            nodata,
            max_cached_files,
            file_expiry,
            file_idle_timeout,
        },
        store_options: options.unwrap_or_default(),
        encoding: EncodingArgs::new(format, quality, background)?,
    };
    let source = spawn(stac.open_source()?).await?.map_err(to_py)?;
    Python::attach(|py| stac.into_py(py, source))
}

/// `value`, any mapping, as JSON text.
fn to_json(value: &Py<PyAny>) -> PyResult<String> {
    Python::attach(|py| {
        let dict = PyDict::new(py);
        dict.update(value.bind(py).cast::<PyMapping>()?)?;
        py.import("json")?.call_method1("dumps", (dict,))?.extract()
    })
}

/// `bands` counted from 1, counted from 0.
fn from_one(bands: &[usize]) -> PyResult<Vec<usize>> {
    bands
        .iter()
        .map(|&band| {
            band.checked_sub(1)
                .ok_or_else(|| PyValueError::new_err("bands are counted from 1"))
        })
        .collect()
}

/// `value` seconds as a duration.
fn seconds(value: f64) -> PyResult<Duration> {
    Duration::try_from_secs_f64(value)
        .map_err(|e| PyValueError::new_err(format!("{value} is not a valid number of seconds: {e}")))
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
#[derive(Clone, Copy, PartialEq, Eq, strum::Display, strum::EnumString, strum::VariantNames)]
#[strum(serialize_all = "lowercase")]
pub enum Format {
    /// Lossless PNG.
    Png,
    /// WebP, lossy or lossless.
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

/// Which STAC items cover a pixel first, by their `datetime`.
#[derive(Clone, Copy, strum::Display, strum::EnumString)]
#[strum(serialize_all = "lowercase")]
pub enum Order {
    /// The newest item is painted first.
    Newest,
    /// The oldest item is painted first.
    Oldest,
}

impl<'py> FromPyObject<'_, 'py> for Order {
    type Error = PyErr;

    fn extract(value: Borrowed<'_, 'py, PyAny>) -> PyResult<Self> {
        let name: String = value.extract()?;
        name.parse()
            .map_err(|_unknown| PyValueError::new_err(format!("order must be \"newest\" or \"oldest\", not {name:?}")))
    }
}

impl<'py> IntoPyObject<'py> for Order {
    type Target = PyString;
    type Output = Bound<'py, PyString>;
    type Error = std::convert::Infallible;

    fn into_pyobject(self, py: Python<'py>) -> Result<Self::Output, Self::Error> {
        Ok(PyString::new(py, &self.to_string()))
    }
}

impl PyStubType for Order {
    fn type_output() -> TypeInfo {
        let mut info = TypeInfo::unqualified("typing.Literal['newest', 'oldest']");
        info.import.insert("typing".into());
        info
    }
}

/// The `format`, `quality` and `background` a source re-encodes its tiles with.
#[derive(Clone, Default)]
struct EncodingArgs {
    format: Option<Format>,
    quality: Option<Quality>,
    background: Option<Background>,
    encoding: Option<TileEncoding>,
}

impl EncodingArgs {
    /// The quality when `quality` is unset, as in Martin.
    const DEFAULT_QUALITY: u8 = 80;

    /// The color JPEG tiles show where they are transparent when `background` is unset, as in Martin.
    const DEFAULT_BACKGROUND: (u8, u8, u8) = (255, 255, 255);

    /// The arguments as given to `open` or `open_mosaic`, rejecting a `quality` or `background` the `format` cannot use.
    fn new(format: Option<Format>, quality: Option<Quality>, background: Option<Background>) -> PyResult<Self> {
        if quality.is_some() && !matches!(format, Some(Format::Webp | Format::Jpeg)) {
            return Err(PyValueError::new_err("quality needs format webp or jpeg"));
        }
        if background.is_some() && format != Some(Format::Jpeg) {
            return Err(PyValueError::new_err("background needs format jpeg"));
        }
        let level = |quality| match quality {
            Some(Quality::Level(level)) => level,
            _ => Self::DEFAULT_QUALITY,
        };
        let encoding = match format {
            None => None,
            Some(Format::Png) => Some(TileEncoding::Png),
            Some(Format::Webp) => Some(TileEncoding::Webp {
                quality: match quality {
                    Some(Quality::Lossless) => None,
                    quality => Some(level(quality)),
                },
            }),
            Some(Format::Jpeg) if matches!(quality, Some(Quality::Lossless)) => {
                return Err(PyValueError::new_err(
                    "JPEG has no lossless mode, so quality must be 1 to 100",
                ));
            }
            Some(Format::Jpeg) => {
                let (r, g, b) = background.map_or(Self::DEFAULT_BACKGROUND, |Background(rgb)| rgb);
                Some(TileEncoding::Jpeg {
                    quality: level(quality),
                    background: [r, g, b],
                })
            }
        };
        Ok(Self {
            format,
            quality,
            background,
            encoding,
        })
    }

    /// The encoding these arguments ask for, or `None` to return tiles as stored.
    fn encoding(&self) -> Option<TileEncoding> {
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
