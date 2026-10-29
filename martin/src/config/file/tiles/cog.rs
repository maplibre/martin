use std::collections::{BTreeMap, HashMap};
use std::fmt::Debug;
use std::mem;
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::stream::{self, StreamExt as _};
use martin_core::tiles::cog::{
    CogMosaicOptions, CogSource, CogStacOptions, CogTileIndexOptions, DEFAULT_LOCATION_FIELD,
    DEFAULT_MAX_CACHED_SEARCHES, DEFAULT_MAX_ITEMS_PER_SEARCH, DEFAULT_SEARCH_EXPIRY, HrefResolver, StacBand,
    StacOrder, StacSearchOptions, TileEncoding,
};
use martin_core::tiles::{BackendSource, BoxedSource};
use martin_core::{ImageQuality, LossyQuality};
use object_store::{ObjectStore, ObjectStoreScheme};
use serde::ser::SerializeMap;
use serde::{Deserialize, Serialize, Serializer};
use tracing::info;
use url::Url;

use crate::config::file::process::ProcessConfig;
use crate::config::file::source_location::SourceLocation;
use crate::config::file::{
    CacheControlHeader, CachePolicy, CollectUnrecognizedKeys, ConfigFileError, ConfigFileResult,
    ConfigurationLivecycleHooks, FileConfig, MAX_CONCURRENT_SOURCE_INITS, ObjectStoreConfig, ResolutionResult,
    SourceBuildResult, TileSourceConfiguration, TileSourceWarning, UnrecognizedValues, resolve_files,
};
#[cfg(feature = "processing")]
use crate::config::file::{ContourProcessConfig, FileConfigSrc, HillshadeProcessConfig};
use crate::config::primitives::IdResolver;

#[derive(Clone, Debug, Deserialize, PartialEq, CollectUnrecognizedKeys)]
#[cfg_attr(feature = "unstable-schemas", derive(schemars::JsonSchema))]
pub struct CogConfig {
    /// Whether `paths` are scanned recursively
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "unstable-schemas", schemars(example = &false))]
    pub recursive: Option<bool>,
    /// Zoom-level bounds for caching the tiles of every COG source without its own `cache`.
    /// Overrides the top-level `cache` bounds.
    #[serde(default, skip_serializing_if = "CachePolicy::is_empty")]
    #[cfg_attr(
        feature = "unstable-schemas",
        schemars(with = "crate::config::file::CachePolicyShape")
    )]
    pub cache: CachePolicy,

    /// How often configured remote objects (`s3://`, `https://`, …) are re-checked for
    /// replacement and remote prefixes are re-listed for additions, replacements, and removals.
    /// Local directories are watched via filesystem events and ignore this setting.
    ///
    /// Supports human-readable formats: "10m", "1h", "30s".
    /// Defaults to "10m". Set to "0s" to disable remote polling and prefix discovery.
    #[serde(default = "default_reload_interval", with = "humantime_serde")]
    #[cfg_attr(
        feature = "unstable-schemas",
        schemars(with = "String", example = &"10m")
    )]
    pub reload_interval: Duration,

    /// Sources that each serve many COG files as one, by source ID.
    #[serde(default)]
    pub mosaics: BTreeMap<String, CogMosaicSource>,

    /// Sources that each serve the COGs a STAC API finds for each tile, by source ID.
    #[serde(default)]
    pub stac: BTreeMap<String, CogStacSource>,

    /// Sources that each serve the COGs a tile index lists, by source ID.
    #[serde(default)]
    pub tile_index: BTreeMap<String, CogTileIndexSource>,

    /// How the COG files under `paths` and `sources` are re-encoded.
    #[serde(flatten)]
    pub encoding: CogEncodingConfig,

    /// Authentication, endpoint, and HTTP client settings for remote COGs.
    #[serde(flatten)]
    pub object_store: ObjectStoreConfig,

    #[serde(flatten, skip_serializing)]
    #[cfg_attr(feature = "unstable-schemas", schemars(skip))]
    pub unrecognized: UnrecognizedValues,
}

/// Default polling interval for
/// [`CogReloader`](crate::config::file::tiles::reload::cog::CogReloader) to check configured remote
/// objects and list remote prefixes. Local directories are notify-driven and ignore this setting.
pub const DEFAULT_RELOAD_INTERVAL: Duration = Duration::from_mins(10);

fn default_reload_interval() -> Duration {
    DEFAULT_RELOAD_INTERVAL
}

/// The format a COG source re-encodes its tiles as.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "unstable-schemas", derive(schemars::JsonSchema))]
pub enum CogTileFormat {
    /// Lossless PNG.
    Png,
    /// WebP, lossy unless `quality` is `lossless`.
    Webp,
    /// JPEG, painted over `background` where tiles are transparent.
    Jpeg,
}

/// An opaque CSS color, read from a string such as `white` or `#336699`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CogBackground(pub [u8; 3]);

impl<'de> Deserialize<'de> for CogBackground {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let css = String::deserialize(deserializer)?;
        match csscolorparser::parse(&css)
            .map_err(serde::de::Error::custom)?
            .to_rgba8()
        {
            [r, g, b, 255] => Ok(Self([r, g, b])),
            _ => Err(serde::de::Error::custom(format_args!(
                "`background` must be an opaque CSS color, not {css:?}"
            ))),
        }
    }
}

impl Serialize for CogBackground {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let [r, g, b] = self.0;
        serializer.collect_str(&format_args!("#{r:02x}{g:02x}{b:02x}"))
    }
}

/// The `format`, `quality` and `background` a COG source re-encodes its tiles with.
#[serde_with::skip_serializing_none]
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, CollectUnrecognizedKeys)]
#[cfg_attr(feature = "unstable-schemas", derive(schemars::JsonSchema))]
pub struct CogEncodingConfig {
    /// The format every tile is re-encoded as, `png`, `webp` or `jpeg`, instead of the source's usual output.
    #[cfg_attr(feature = "unstable-schemas", schemars(example = &None::<CogTileFormat>))]
    pub format: Option<CogTileFormat>,
    /// How lossy `webp` and `jpeg` tiles are, from 1 (smallest) to 100 (best) or `lossless` for `webp`, 80 when unset.
    #[cfg_attr(feature = "unstable-schemas", schemars(example = &None::<ImageQuality>))]
    pub quality: Option<ImageQuality>,
    /// The CSS color painted under transparent `jpeg` pixels, white when unset.
    #[cfg_attr(feature = "unstable-schemas", schemars(with = "Option<String>"))]
    pub background: Option<CogBackground>,
}

impl CogEncodingConfig {
    /// The encoding these settings select, or `None` to serve tiles as the source would.
    fn resolve(self) -> Result<Option<TileEncoding>, CogEncodingError> {
        let Self {
            format,
            quality,
            background,
        } = self;
        if background.is_some() && format != Some(CogTileFormat::Jpeg) {
            return Err(CogEncodingError::BackgroundWithoutJpeg);
        }
        let lossy = match quality {
            Some(ImageQuality::Lossy(quality)) => quality,
            _ => LossyQuality::default(),
        }
        .get();
        Ok(Some(match (format, quality) {
            (None, None) => return Ok(None),
            (None | Some(CogTileFormat::Png), Some(_)) => return Err(CogEncodingError::QualityWithoutLossyFormat),
            (Some(CogTileFormat::Png), None) => TileEncoding::Png,
            (Some(CogTileFormat::Webp), Some(ImageQuality::Lossless)) => TileEncoding::Webp { quality: None },
            (Some(CogTileFormat::Webp), _) => TileEncoding::Webp { quality: Some(lossy) },
            (Some(CogTileFormat::Jpeg), Some(ImageQuality::Lossless)) => return Err(CogEncodingError::LosslessJpeg),
            (Some(CogTileFormat::Jpeg), _) => TileEncoding::Jpeg {
                quality: lossy,
                background: background.map_or([255; 3], |CogBackground(rgb)| rgb),
            },
        }))
    }

    /// Writes the set keys into `map`, for configs that serialize by hand.
    fn serialize_entries<M: SerializeMap>(self, map: &mut M) -> Result<(), M::Error> {
        if let Some(format) = self.format {
            map.serialize_entry("format", &format)?;
        }
        if let Some(quality) = self.quality {
            map.serialize_entry("quality", &quality)?;
        }
        if let Some(background) = self.background {
            map.serialize_entry("background", &background)?;
        }
        Ok(())
    }
}

/// Why the `format`, `quality` and `background` of a COG source do not go together.
#[derive(thiserror::Error, Debug, Clone, PartialEq, Eq)]
pub enum CogEncodingError {
    /// `quality` is set without a lossy format.
    #[error("`quality` needs `format: webp` or `format: jpeg`")]
    QualityWithoutLossyFormat,
    /// `quality: lossless` is set for JPEG.
    #[error("JPEG has no lossless mode, so `quality` must be 1 to 100")]
    LosslessJpeg,
    /// `background` is set without JPEG.
    #[error("`background` needs `format: jpeg`")]
    BackgroundWithoutJpeg,
    /// `format` is set on a source that converts its tiles.
    #[cfg(feature = "processing")]
    #[error(
        "`format` cannot be set with `convert_to_hillshade` or `convert_to_contour`, which encode tiles themselves"
    )]
    FormatWithConversion,
}

/// Default for [`CogMosaicSource::max_cached_files`].
pub const DEFAULT_MOSAIC_CACHED_FILES: u64 = 1000;

/// Default for [`CogMosaicSource::file_expiry`].
pub const DEFAULT_MOSAIC_FILE_EXPIRY: Duration = Duration::from_mins(10);

/// A source that serves many COG files as one, each covering exactly one tile at `zoom`.
///
/// Files are opened on first use and kept in a bounded cache; tiles without a file are empty.
/// One file is found by listing the location at startup to learn the tile size and format.
#[serde_with::skip_serializing_none]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, CollectUnrecognizedKeys)]
#[cfg_attr(feature = "unstable-schemas", derive(schemars::JsonSchema))]
pub struct CogMosaicSource {
    /// Where the files are, local or remote, naming each file by its tile at `zoom`:
    /// `{quadkey}`, or `{x}` and `{y}` with an optional `{z}`, e.g. `s3://bucket/ortho/{quadkey}.tif`
    /// or `/data/ortho/{z}/{x}/{y}.tif`. `{-y}` is the row counted from the bottom (TMS).
    pub path: String,
    /// The zoom level at which each file covers exactly one tile. It is also the lowest zoom served.
    #[cfg_attr(feature = "unstable-schemas", schemars(example = &16))]
    pub zoom: u8,
    /// The highest zoom served. When unset, the highest zoom of the file opened at startup.
    pub maxzoom: Option<u8>,
    /// How many opened files (and known-missing ones) are kept at once. Defaults to 1000.
    #[cfg_attr(feature = "unstable-schemas", schemars(example = &DEFAULT_MOSAIC_CACHED_FILES))]
    pub max_cached_files: Option<u64>,
    /// How long an opened file is kept before it is opened again, which picks up replaced and
    /// added files. Supports human-readable formats: "10m", "1h", "30s". Defaults to "10m".
    #[serde(default, with = "humantime_serde")]
    #[cfg_attr(
        feature = "unstable-schemas",
        schemars(with = "Option<String>", example = &"10m")
    )]
    pub file_expiry: Option<Duration>,
    /// How long an opened file is kept without being used. Supports human-readable formats:
    /// "10m", "1h", "30s". Defaults to none: files are kept until `file_expiry`.
    #[serde(default, with = "humantime_serde")]
    #[cfg_attr(
        feature = "unstable-schemas",
        schemars(with = "Option<String>", example = &"5m")
    )]
    pub file_idle_timeout: Option<Duration>,
    /// Zoom-level bounds for tile caching.
    #[serde(default, skip_serializing_if = "CachePolicy::is_empty")]
    #[cfg_attr(
        feature = "unstable-schemas",
        schemars(with = "crate::config::file::CachePolicyShape")
    )]
    pub cache: CachePolicy,
    /// `Cache-Control` response header for this source.
    /// Overrides the top-level `cache_control` default.
    #[serde(default)]
    #[cfg_attr(feature = "unstable-schemas", schemars(with = "Option<String>"))]
    pub cache_control: Option<CacheControlHeader>,
    /// How every tile is re-encoded.
    #[serde(flatten)]
    pub encoding: CogEncodingConfig,
    /// Hillshade settings for this source.
    ///
    /// Present means the source serves Mapzen *normal* tiles and Martin should bake a hillshade from them.
    /// See the hillshade documentation for the knobs.
    #[cfg(feature = "processing")]
    #[serde(default)]
    pub convert_to_hillshade: Option<HillshadeProcessConfig>,
    /// Trace contour lines from this source's tiles.
    ///
    /// Present means the source serves Mapzen *Terrarium* elevation tiles and Martin should trace contours from them.
    /// See the contour documentation for the knobs.
    #[cfg(feature = "processing")]
    #[serde(default)]
    pub convert_to_contour: Option<ContourProcessConfig>,

    #[serde(flatten, skip_serializing)]
    #[cfg_attr(feature = "unstable-schemas", schemars(skip))]
    pub unrecognized: UnrecognizedValues,
}

impl CogMosaicSource {
    /// The per-source raster processing and `cache_control` settings.
    pub(crate) fn process(&self) -> ProcessConfig {
        ProcessConfig {
            convert_to_mlt: None,
            convert_to_mvt: None,
            cache_control: self.cache_control.clone(),
            #[cfg(feature = "processing")]
            convert_to_hillshade: self.convert_to_hillshade.clone(),
            #[cfg(feature = "processing")]
            convert_to_contour: self.convert_to_contour.clone(),
        }
    }

    /// The encoding `encoding` selects, which `convert_to_hillshade` and `convert_to_contour` rule out.
    fn tile_encoding(&self) -> Result<Option<TileEncoding>, CogEncodingError> {
        #[cfg(feature = "processing")]
        if self.encoding.format.is_some() && (self.convert_to_hillshade.is_some() || self.convert_to_contour.is_some())
        {
            return Err(CogEncodingError::FormatWithConversion);
        }
        self.encoding.resolve()
    }

    fn options(&self) -> CogMosaicOptions {
        CogMosaicOptions {
            file_zoom: self.zoom,
            max_zoom: self.maxzoom,
            max_cached_files: self.max_cached_files.unwrap_or(DEFAULT_MOSAIC_CACHED_FILES),
            file_expiry: self.file_expiry.unwrap_or(DEFAULT_MOSAIC_FILE_EXPIRY),
            file_idle_timeout: self.file_idle_timeout,
        }
    }

    /// This mosaic with credentials, query and fragment removed from a remote `path`.
    fn sanitized(&self) -> Self {
        Self {
            path: sanitized_mosaic_path(&self.path),
            ..self.clone()
        }
    }
}

/// A source that serves the COGs a STAC API finds for each tile, reprojected onto Web Mercator.
///
/// Each search covers one tile at `search_zoom` and serves every tile below it. A tile is painted
/// from the items found in `order`: a pixel takes the first item's data for it, so later items only
/// fill what earlier ones leave empty. Files may be in Web Mercator or a UTM zone, and are opened on
/// first use. Tiles are PNG unless `format` says otherwise, transparent where no item has data.
#[serde_with::skip_serializing_none]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, CollectUnrecognizedKeys)]
#[cfg_attr(feature = "unstable-schemas", derive(schemars::JsonSchema))]
pub struct CogStacSource {
    /// The root of the STAC API, e.g. `https://earth-search.aws.element84.com/v1`.
    /// Items are searched at its `/search` endpoint.
    pub url: String,
    /// The collection to search, e.g. `naip`.
    pub collection: String,
    /// The key of the asset to read from each item, e.g. `image` or `visual`.
    pub asset: String,
    /// The bands to serve, by number (counted from 1) or by `eo:bands` `common_name` or `name`:
    /// one for gray tiles, three for RGB tiles. When unset, `[red, green, blue]`, or the first
    /// three bands if the asset does not name its bands.
    #[cfg_attr(feature = "unstable-schemas", schemars(example = &["red", "green", "blue"]))]
    pub bands: Option<Vec<StacBandConfig>>,
    /// A STAC `datetime` the items must fall in: one instant or an interval, open-ended with `..`,
    /// e.g. `2020-01-01T00:00:00Z/..`.
    pub datetime: Option<String>,
    /// A STAC query extension filter, sent as is, e.g. `{"naip:state": {"eq": "ca"}}`.
    #[cfg_attr(
        feature = "unstable-schemas",
        schemars(with = "Option<serde_json::Map<String, serde_json::Value>>")
    )]
    pub query: Option<serde_json::Map<String, serde_json::Value>>,
    /// Which items cover a pixel first, by `datetime`: `newest` or `oldest`. When unset, `newest`.
    pub order: Option<StacOrderConfig>,
    /// The lowest zoom served. Lower zooms are empty.
    #[cfg_attr(feature = "unstable-schemas", schemars(example = &12))]
    pub minzoom: u8,
    /// The highest zoom served. When unset, the zoom at which the first item found is fully detailed.
    pub maxzoom: Option<u8>,
    /// Each search covers one tile at this zoom and serves every tile below it. It cannot be above
    /// `minzoom`. When unset, `minzoom`.
    pub search_zoom: Option<u8>,
    /// How long a search result is kept before searching again. Supports human-readable formats:
    /// "1d", "1h", "30m". When unset, "1d".
    #[serde(default, with = "humantime_serde")]
    #[cfg_attr(
        feature = "unstable-schemas",
        schemars(with = "Option<String>", example = &"1d")
    )]
    pub search_expiry: Option<Duration>,
    /// How many search results are kept at once. When unset, 10000.
    pub max_cached_searches: Option<u64>,
    /// How many items one search reads, newest (or oldest) first, counting those skipped for
    /// lacking the asset; the rest are not painted and a warning is logged. When unset, 1000.
    pub max_items_per_search: Option<NonZeroUsize>,
    /// The sample value of pixels without data, overriding each file's own nodata value.
    pub nodata: Option<u8>,
    /// How many opened files are kept at once. When unset, 1000.
    pub max_cached_files: Option<u64>,
    /// How long an opened file is kept before it is opened again. Supports human-readable formats:
    /// "10m", "1h", "30s". When unset, "10m".
    #[serde(default, with = "humantime_serde")]
    #[cfg_attr(
        feature = "unstable-schemas",
        schemars(with = "Option<String>", example = &"10m")
    )]
    pub file_expiry: Option<Duration>,
    /// How long an opened file is kept without being used. Supports human-readable formats:
    /// "10m", "1h", "30s". When unset, files are kept until `file_expiry`.
    #[serde(default, with = "humantime_serde")]
    #[cfg_attr(
        feature = "unstable-schemas",
        schemars(with = "Option<String>", example = &"5m")
    )]
    pub file_idle_timeout: Option<Duration>,
    /// Zoom-level bounds for tile caching. The tile cache does not expire with `search_expiry`,
    /// so pair this with the top-level `cache.expiry`. When unset, tiles are not cached; only
    /// searches, opened files and decoded blocks are.
    #[serde(default, skip_serializing_if = "CachePolicy::is_empty")]
    #[cfg_attr(
        feature = "unstable-schemas",
        schemars(with = "crate::config::file::CachePolicyShape")
    )]
    pub cache: CachePolicy,
    /// `Cache-Control` response header for this source.
    /// Overrides the top-level `cache_control` default.
    #[serde(default)]
    #[cfg_attr(feature = "unstable-schemas", schemars(with = "Option<String>"))]
    pub cache_control: Option<CacheControlHeader>,
    /// How every tile is re-encoded.
    #[serde(flatten)]
    pub encoding: CogEncodingConfig,

    #[serde(flatten, skip_serializing)]
    #[cfg_attr(feature = "unstable-schemas", schemars(skip))]
    pub unrecognized: UnrecognizedValues,
}

/// One band of a STAC asset: its number, counted from 1, or its `eo:bands` name.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "unstable-schemas", derive(schemars::JsonSchema))]
#[serde(untagged)]
pub enum StacBandConfig {
    /// The band's number, counted from 1.
    Number(usize),
    /// The band's `common_name` or `name` in `eo:bands`, e.g. `red`.
    Name(String),
}

/// Which STAC items cover a pixel first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "unstable-schemas", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum StacOrderConfig {
    /// The newest item is painted first.
    Newest,
    /// The oldest item is painted first.
    Oldest,
}

impl CogStacSource {
    /// The per-source `cache_control` setting.
    pub(crate) fn process(&self) -> ProcessConfig {
        ProcessConfig {
            convert_to_mlt: None,
            convert_to_mvt: None,
            cache_control: self.cache_control.clone(),
            #[cfg(feature = "processing")]
            convert_to_hillshade: None,
            #[cfg(feature = "processing")]
            convert_to_contour: None,
        }
    }

    /// The options [`CogSource::new_stac`] takes.
    fn options(&self) -> CogStacOptions {
        CogStacOptions {
            search: StacSearchOptions {
                url: self.url.clone(),
                user_agent: concat!("martin/", env!("CARGO_PKG_VERSION")).to_owned(),
                collection: self.collection.clone(),
                asset: self.asset.clone(),
                datetime: self.datetime.clone(),
                query: self.query.clone(),
                order: match self.order {
                    Some(StacOrderConfig::Oldest) => StacOrder::Oldest,
                    Some(StacOrderConfig::Newest) | None => StacOrder::Newest,
                },
                search_zoom: self.search_zoom.unwrap_or(self.minzoom),
                max_items_per_search: self.max_items_per_search.unwrap_or(DEFAULT_MAX_ITEMS_PER_SEARCH),
                search_expiry: self.search_expiry.unwrap_or(DEFAULT_SEARCH_EXPIRY),
                max_cached_searches: self.max_cached_searches.unwrap_or(DEFAULT_MAX_CACHED_SEARCHES),
            },
            min_zoom: self.minzoom,
            max_zoom: self.maxzoom,
            bands: self.bands.as_ref().map(|bands| {
                bands
                    .iter()
                    .map(|band| match band {
                        StacBandConfig::Number(number) => StacBand::Number(*number),
                        StacBandConfig::Name(name) => StacBand::Name(name.clone()),
                    })
                    .collect()
            }),
            nodata: self.nodata,
            max_cached_files: self.max_cached_files.unwrap_or(DEFAULT_MOSAIC_CACHED_FILES),
            file_expiry: self.file_expiry.unwrap_or(DEFAULT_MOSAIC_FILE_EXPIRY),
            file_idle_timeout: self.file_idle_timeout,
        }
    }
}

/// A source that serves the COGs a tile index lists, reprojected onto Web Mercator.
///
/// The index is a `GeoPackage` with one row per file: its footprint in `EPSG:4326` and its location,
/// as written by `gdaltindex -f GPKG -t_srs EPSG:4326`. It is read into memory at startup. A tile is
/// painted from the files whose footprints touch it, in the index's row order: a pixel takes the first
/// file's data for it, so later files only fill what earlier ones leave empty. Files may be in Web
/// Mercator or a UTM zone, and are opened on first use. Tiles are PNG unless `format` says otherwise,
/// transparent where no file has data.
#[serde_with::skip_serializing_none]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, CollectUnrecognizedKeys)]
#[cfg_attr(feature = "unstable-schemas", derive(schemars::JsonSchema))]
pub struct CogTileIndexSource {
    /// The `GeoPackage` tile index, e.g. `/data/naip_ca_2020.gpkg`.
    pub path: PathBuf,
    /// The feature table to read. When unset, the only feature table in the file.
    pub layer: Option<String>,
    /// The column holding each file's location: a URL, a GDAL `/vsis3/`, `/vsigs/`, `/vsiaz/` or
    /// `/vsicurl/` path, or a local path relative to the index file. When unset, `location`.
    pub location_field: Option<String>,
    /// The bands to serve, by number (counted from 1): one for gray tiles, three for RGB tiles.
    /// When unset, `[1, 2, 3]`.
    #[cfg_attr(feature = "unstable-schemas", schemars(example = &[1, 2, 3]))]
    pub bands: Option<Vec<NonZeroUsize>>,
    /// The lowest zoom served. Lower zooms are empty.
    #[cfg_attr(feature = "unstable-schemas", schemars(example = &16))]
    pub minzoom: u8,
    /// The highest zoom served. When unset, the zoom at which the first file is fully detailed.
    pub maxzoom: Option<u8>,
    /// The sample value of pixels without data, overriding each file's own nodata value.
    pub nodata: Option<u8>,
    /// How many opened files are kept at once. When unset, 1000.
    pub max_cached_files: Option<u64>,
    /// How long an opened file is kept before it is opened again. Supports human-readable formats:
    /// "10m", "1h", "30s". When unset, "10m".
    #[serde(default, with = "humantime_serde")]
    #[cfg_attr(
        feature = "unstable-schemas",
        schemars(with = "Option<String>", example = &"10m")
    )]
    pub file_expiry: Option<Duration>,
    /// How long an opened file is kept without being used. Supports human-readable formats:
    /// "10m", "1h", "30s". When unset, files are kept until `file_expiry`.
    #[serde(default, with = "humantime_serde")]
    #[cfg_attr(
        feature = "unstable-schemas",
        schemars(with = "Option<String>", example = &"5m")
    )]
    pub file_idle_timeout: Option<Duration>,
    /// Zoom-level bounds for tile caching.
    #[serde(default, skip_serializing_if = "CachePolicy::is_empty")]
    #[cfg_attr(
        feature = "unstable-schemas",
        schemars(with = "crate::config::file::CachePolicyShape")
    )]
    pub cache: CachePolicy,
    /// `Cache-Control` response header for this source.
    /// Overrides the top-level `cache_control` default.
    #[serde(default)]
    #[cfg_attr(feature = "unstable-schemas", schemars(with = "Option<String>"))]
    pub cache_control: Option<CacheControlHeader>,
    /// How every tile is re-encoded.
    #[serde(flatten)]
    pub encoding: CogEncodingConfig,

    #[serde(flatten, skip_serializing)]
    #[cfg_attr(feature = "unstable-schemas", schemars(skip))]
    pub unrecognized: UnrecognizedValues,
}

impl CogTileIndexSource {
    /// The per-source `cache_control` setting.
    pub(crate) fn process(&self) -> ProcessConfig {
        ProcessConfig {
            convert_to_mlt: None,
            convert_to_mvt: None,
            cache_control: self.cache_control.clone(),
            #[cfg(feature = "processing")]
            convert_to_hillshade: None,
            #[cfg(feature = "processing")]
            convert_to_contour: None,
        }
    }

    /// The options [`CogSource::new_tile_index`] takes.
    fn options(&self) -> CogTileIndexOptions {
        CogTileIndexOptions {
            path: self.path.clone(),
            layer: self.layer.clone(),
            location_field: self
                .location_field
                .clone()
                .unwrap_or_else(|| DEFAULT_LOCATION_FIELD.to_owned()),
            min_zoom: self.minzoom,
            max_zoom: self.maxzoom,
            bands: self.bands.as_ref().map_or_else(
                || vec![0, 1, 2],
                |bands| bands.iter().map(|band| band.get() - 1).collect(),
            ),
            nodata: self.nodata,
            max_cached_files: self.max_cached_files.unwrap_or(DEFAULT_MOSAIC_CACHED_FILES),
            file_expiry: self.file_expiry.unwrap_or(DEFAULT_MOSAIC_FILE_EXPIRY),
            file_idle_timeout: self.file_idle_timeout,
        }
    }
}

/// A mosaic path with credentials, query and fragment removed from its remote location.
fn sanitized_mosaic_path(path: &str) -> String {
    let (location, template) = split_mosaic_path(path);
    match SourceLocation::classify(&location) {
        Ok(SourceLocation::ObjectStore(url) | SourceLocation::Http(url)) => {
            format!("{}{template}", sanitized_url(&url))
        }
        Ok(SourceLocation::Local(_)) | Err(_) => path.to_owned(),
    }
}

/// Splits a mosaic path into the location of its files and the key template under it,
/// at the last `/` before the first placeholder. A URL query stays with the location.
fn split_mosaic_path(path: &str) -> (String, &str) {
    let (path, query) = path
        .split_once('?')
        .map_or((path, None), |(path, query)| (path, Some(query)));
    let before_placeholder = path.split('{').next().unwrap_or_default();
    let split = before_placeholder.rfind(['/', '\\']).map_or(0, |i| i + 1);
    let (location, template) = path.split_at(split);
    let location = match query {
        Some(query) => format!("{location}?{query}"),
        None => location.to_owned(),
    };
    (location, template)
}

impl Default for CogConfig {
    fn default() -> Self {
        Self {
            recursive: None,
            cache: CachePolicy::default(),
            reload_interval: DEFAULT_RELOAD_INTERVAL,
            mosaics: BTreeMap::new(),
            stac: BTreeMap::new(),
            tile_index: BTreeMap::new(),
            encoding: CogEncodingConfig::default(),
            object_store: ObjectStoreConfig::default(),
            unrecognized: UnrecognizedValues::default(),
        }
    }
}

impl Serialize for CogConfig {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut map = serializer.serialize_map(None)?;
        if let Some(recursive) = self.recursive {
            map.serialize_entry("recursive", &recursive)?;
        }
        if !self.cache.is_empty() {
            map.serialize_entry("cache", &self.cache)?;
        }
        if self.reload_interval != DEFAULT_RELOAD_INTERVAL {
            map.serialize_entry("reload_interval", &humantime_serde::Serde::from(&self.reload_interval))?;
        }
        if !self.mosaics.is_empty() {
            let mosaics: BTreeMap<_, _> = self
                .mosaics
                .iter()
                .map(|(id, mosaic)| (id, mosaic.sanitized()))
                .collect();
            map.serialize_entry("mosaics", &mosaics)?;
        }
        if !self.stac.is_empty() {
            map.serialize_entry("stac", &self.stac)?;
        }
        if !self.tile_index.is_empty() {
            map.serialize_entry("tile_index", &self.tile_index)?;
        }
        self.encoding.serialize_entries(&mut map)?;
        self.object_store.serialize_entries(&mut map)?;
        map.end()
    }
}

impl ConfigurationLivecycleHooks for CogConfig {
    async fn finalize(&mut self) -> ConfigFileResult<()> {
        self.encoding.resolve().map_err(invalid_encoding("cog"))?;
        for (id, mosaic) in &self.mosaics {
            mosaic.tile_encoding().map_err(invalid_encoding(id))?;
        }
        for (id, stac) in &self.stac {
            stac.encoding.resolve().map_err(invalid_encoding(id))?;
        }
        for (id, index) in &self.tile_index {
            index.encoding.resolve().map_err(invalid_encoding(id))?;
        }
        self.object_store.prepare(&mut self.unrecognized, "cog");
        self.object_store.finalize_runtime("cog").await;
        Ok(())
    }
}

impl TileSourceConfiguration for CogConfig {
    fn parse_urls() -> bool {
        true
    }

    fn cache(&self) -> CachePolicy {
        self.cache.clone()
    }

    async fn new_sources(&self, id: String, path: PathBuf, cache: CachePolicy) -> SourceBuildResult<BackendSource> {
        let encoding = self.encoding.resolve().map_err(invalid_encoding(&id))?;
        let cog = CogSource::new(id, path, cache.zoom()).await?;
        Ok(BackendSource::Cog(cog.with_encoding(encoding)))
    }

    async fn new_sources_url(&self, id: String, url: Url, cache: CachePolicy) -> SourceBuildResult<BackendSource> {
        let (store, path) = self
            .object_store
            .parse_url_opts(&url)
            .map_err(|e| ConfigFileError::ObjectStoreUrlParsing(e, id.clone()))?;
        let encoding = self.encoding.resolve().map_err(invalid_encoding(&id))?;
        let source = CogSource::new_object_store(id, Arc::from(store), path, sanitized_url(&url), cache.zoom()).await?;
        Ok(BackendSource::Cog(source.with_encoding(encoding)))
    }
}

#[cfg(feature = "processing")]
impl CogConfig {
    /// Rejects a `format` for the COG files when one of the `sources` converts its tiles.
    pub fn check_file_conversions(config: &FileConfig<Self>) -> ConfigFileResult<()> {
        if config.custom.encoding.format.is_none() {
            return Ok(());
        }
        for (id, source) in &config.sources {
            if let FileConfigSrc::Obj(source) = source
                && (source.convert_to_hillshade.is_some() || source.convert_to_contour.is_some())
            {
                return Err(invalid_encoding(id)(CogEncodingError::FormatWithConversion));
            }
        }
        Ok(())
    }
}

/// Names `source_id` in an error about its encoding.
fn invalid_encoding(source_id: &str) -> impl FnOnce(CogEncodingError) -> ConfigFileError + '_ {
    move |reason| ConfigFileError::InvalidCogEncoding {
        source_id: source_id.to_owned(),
        reason,
    }
}

impl CogConfig {
    /// Opens every configured mosaic, at most [`MAX_CONCURRENT_SOURCE_INITS`] at once.
    /// A mosaic that fails to open is reported as a warning and stays in the config.
    async fn resolve_mosaics(
        &mut self,
        idr: &IdResolver,
        default_cache: CachePolicy,
    ) -> (Vec<BoxedSource>, Vec<TileSourceWarning>) {
        let planned: Vec<_> = mem::take(&mut self.mosaics)
            .into_iter()
            .map(|(id, mosaic)| (idr.resolve(&id, sanitized_mosaic_path(&mosaic.path)), mosaic))
            .collect();
        let this = &*self;
        let default_cache = &default_cache;
        let opened = stream::iter(planned)
            .map(|(id, mosaic)| async move {
                let result = this.new_mosaic_source(id.clone(), &mosaic, default_cache).await;
                (id, mosaic, result)
            })
            .buffered(MAX_CONCURRENT_SOURCE_INITS)
            .collect::<Vec<_>>()
            .await;

        let mut results = Vec::new();
        let mut warnings = Vec::new();
        for (id, mosaic, result) in opened {
            match result {
                Ok(src) => {
                    info!(
                        source.id = %id,
                        source.mosaic = %sanitized_mosaic_path(&mosaic.path),
                        "Configured mosaic source"
                    );
                    results.push(src.boxed_on(None));
                }
                Err(err) => warnings.push(TileSourceWarning::SourceError {
                    source_id: id.clone(),
                    error: err.to_string(),
                }),
            }
            self.mosaics.insert(id, mosaic);
        }
        (results, warnings)
    }

    /// Opens one mosaic, whose `path` names its files by their tile.
    async fn new_mosaic_source(
        &self,
        id: String,
        mosaic: &CogMosaicSource,
        default_cache: &CachePolicy,
    ) -> SourceBuildResult<BackendSource> {
        let encoding = mosaic.tile_encoding().map_err(invalid_encoding(&id))?;
        let (location, template) = split_mosaic_path(&mosaic.path);
        let options = mosaic.options();
        let cache = mosaic.cache.or(default_cache);
        let source = match SourceLocation::classify(&location)? {
            SourceLocation::ObjectStore(url) | SourceLocation::Http(url) => {
                let (store, prefix) = self
                    .object_store
                    .parse_url_opts(&url)
                    .map_err(|e| ConfigFileError::ObjectStoreUrlParsing(e, id.clone()))?;
                CogSource::new_mosaic_object_store(
                    id,
                    Arc::from(store),
                    prefix,
                    template,
                    sanitized_mosaic_path(&mosaic.path),
                    options,
                    cache.zoom(),
                )
                .await?
            }
            SourceLocation::Local(root) => {
                let root = if root.as_os_str().is_empty() {
                    PathBuf::from(".")
                } else {
                    root
                };
                CogSource::new_mosaic_dir(id, root, template, options, cache.zoom()).await?
            }
        };
        Ok(BackendSource::Cog(source.with_encoding(encoding)))
    }
}

impl CogConfig {
    /// Opens every configured STAC source, at most [`MAX_CONCURRENT_SOURCE_INITS`] at once.
    /// A source that fails to open is reported as a warning and stays in the config.
    async fn resolve_stac(
        &mut self,
        idr: &IdResolver,
        default_cache: CachePolicy,
    ) -> (Vec<BoxedSource>, Vec<TileSourceWarning>) {
        let planned: Vec<_> = mem::take(&mut self.stac)
            .into_iter()
            .map(|(id, stac)| {
                let location = format!("{}/collections/{}", stac.url, stac.collection);
                (idr.resolve(&id, location), stac)
            })
            .collect();
        let resolve = self.href_resolver();
        let default_cache = &default_cache;
        let opened = stream::iter(planned)
            .map(|(id, stac)| {
                let resolve = Arc::clone(&resolve);
                async move {
                    let cache = if stac.cache.is_empty() {
                        CachePolicy::disabled()
                    } else {
                        stac.cache.or(default_cache)
                    };
                    let result: SourceBuildResult<CogSource> = async {
                        let encoding = stac.encoding.resolve().map_err(invalid_encoding(&id))?;
                        let source = CogSource::new_stac(id.clone(), stac.options(), resolve, cache.zoom()).await?;
                        Ok(source.with_encoding(encoding))
                    }
                    .await;
                    (id, stac, result)
                }
            })
            .buffered(MAX_CONCURRENT_SOURCE_INITS)
            .collect::<Vec<_>>()
            .await;

        let mut results = Vec::new();
        let mut warnings = Vec::new();
        for (id, stac, result) in opened {
            match result {
                Ok(src) => {
                    info!(
                        source.id = %id,
                        source.stac = %stac.url,
                        source.collection = %stac.collection,
                        "Configured STAC source"
                    );
                    results.push(BackendSource::Cog(src).boxed_on(None));
                }
                Err(err) => warnings.push(TileSourceWarning::SourceError {
                    source_id: id.clone(),
                    error: err.to_string(),
                }),
            }
            self.stac.insert(id, stac);
        }
        (results, warnings)
    }

    /// Opens every configured tile index source, at most [`MAX_CONCURRENT_SOURCE_INITS`] at once.
    /// A source that fails to open is reported as a warning and stays in the config.
    async fn resolve_tile_index(
        &mut self,
        idr: &IdResolver,
        default_cache: CachePolicy,
    ) -> (Vec<BoxedSource>, Vec<TileSourceWarning>) {
        let planned: Vec<_> = mem::take(&mut self.tile_index)
            .into_iter()
            .map(|(id, index)| (idr.resolve(&id, index.path.display().to_string()), index))
            .collect();
        let resolve = self.href_resolver();
        let default_cache = &default_cache;
        let opened = stream::iter(planned)
            .map(|(id, index)| {
                let resolve = &resolve;
                async move {
                    let cache = index.cache.or(default_cache);
                    let result: SourceBuildResult<CogSource> = async {
                        let encoding = index.encoding.resolve().map_err(invalid_encoding(&id))?;
                        let source =
                            CogSource::new_tile_index(id.clone(), index.options(), resolve, cache.zoom()).await?;
                        Ok(source.with_encoding(encoding))
                    }
                    .await;
                    (id, index, result)
                }
            })
            .buffered(MAX_CONCURRENT_SOURCE_INITS)
            .collect::<Vec<_>>()
            .await;

        let mut results = Vec::new();
        let mut warnings = Vec::new();
        for (id, index, result) in opened {
            match result {
                Ok(src) => {
                    info!(
                        source.id = %id,
                        source.tile_index = %index.path.display(),
                        "Configured tile index source"
                    );
                    results.push(BackendSource::Cog(src).boxed_on(None));
                }
                Err(err) => warnings.push(TileSourceWarning::SourceError {
                    source_id: id.clone(),
                    error: err.to_string(),
                }),
            }
            self.tile_index.insert(id, index);
        }
        (results, warnings)
    }

    /// Builds the stores STAC asset hrefs and tile index locations are read from, with this config's object-store settings,
    /// one per host and first path segment, which holds the bucket or container of path-style URLs.
    /// An href with a query (e.g. a presigned URL) gets a store of its own.
    fn href_resolver(&self) -> HrefResolver {
        let object_store = self.object_store.clone();
        let stores: Mutex<HashMap<String, Arc<dyn ObjectStore>>> = Mutex::default();
        Arc::new(move |href: &str| {
            let url = Url::parse(href)?;
            if url.query().is_some() {
                let (store, path) = object_store.parse_url_opts(&url)?;
                return Ok((Arc::from(store), path));
            }
            let first_segment = url
                .path_segments()
                .and_then(|mut segments| segments.next())
                .unwrap_or("");
            let key = format!("{}/{first_segment}", &url[..url::Position::BeforePath]);
            let mut stores = stores
                .lock()
                .map_err(|_poisoned| "the object store cache is poisoned")?;
            if let Some(store) = stores.get(&key) {
                let (_, path) = ObjectStoreScheme::parse(&url)?;
                return Ok((Arc::clone(store), path));
            }
            let (store, path) = object_store.parse_url_opts(&url)?;
            let store: Arc<dyn ObjectStore> = Arc::from(store);
            stores.insert(key, Arc::clone(&store));
            Ok((store, path))
        })
    }
}

/// Resolves the COG sources and files, then the mosaics, STAC and tile index sources.
pub async fn resolve_cog(
    config: &mut FileConfig<CogConfig>,
    idr: &IdResolver,
    default_cache: CachePolicy,
) -> ResolutionResult {
    let (mut sources, mut warnings) = resolve_files(config, idr, &["tif", "tiff"], default_cache.clone(), None).await?;
    let default_cache = config.cache_or(&default_cache);
    let (mosaics, mosaic_warnings) = config.custom.resolve_mosaics(idr, default_cache.clone()).await;
    sources.extend(mosaics);
    warnings.extend(mosaic_warnings);
    let (stac, stac_warnings) = config.custom.resolve_stac(idr, default_cache.clone()).await;
    sources.extend(stac);
    warnings.extend(stac_warnings);
    let (indexed, index_warnings) = config.custom.resolve_tile_index(idr, default_cache).await;
    sources.extend(indexed);
    warnings.extend(index_warnings);
    Ok((sources, warnings))
}

fn sanitized_url(url: &Url) -> String {
    let mut url = url.clone();
    let _ = url.set_username("");
    let _ = url.set_password(None);
    url.set_query(None);
    url.set_fragment(None);
    url.to_string()
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::split_mosaic_path;
    use crate::config::file::{CollectUnrecognizedKeys as _, Config};
    use crate::config::primitives::IdResolver;
    use crate::srv::RESERVED_KEYWORDS;

    #[rstest]
    #[case("s3://bucket/ortho/{quadkey}.tif", "s3://bucket/ortho/", "{quadkey}.tif")]
    #[case("s3://bucket/{z}/{x}/{y}.tif", "s3://bucket/", "{z}/{x}/{y}.tif")]
    #[case(
        "https://host/ortho/{quadkey}.tif?sig=abc",
        "https://host/ortho/?sig=abc",
        "{quadkey}.tif"
    )]
    #[case("/data/ortho/tile_{x}_{y}.tif", "/data/ortho/", "tile_{x}_{y}.tif")]
    #[case("{quadkey}.tif", "", "{quadkey}.tif")]
    #[case("/data/plain.tif", "/data/", "plain.tif")]
    fn splits_a_mosaic_path_at_its_first_placeholder(
        #[case] path: &str,
        #[case] location: &str,
        #[case] template: &str,
    ) {
        assert_eq!(split_mosaic_path(path), (location.to_owned(), template));
    }

    async fn resolve(yaml: &str) -> (Config, Vec<String>, serde_json::Value) {
        let mut config: Config = serde_saphyr::from_str(yaml).unwrap();
        config.finalize().await.unwrap();
        let state = config.resolve(&IdResolver::new(RESERVED_KEYWORDS)).await.unwrap();
        let mut ids = state.tile_manager.tile_sources().source_names();
        ids.sort();
        let saved = serde_saphyr::to_string(&config.with_catalog(&state.tile_manager)).unwrap();
        let saved: serde_json::Value = serde_saphyr::from_str(&saved).unwrap();
        (config, ids, saved["cog"].clone())
    }

    #[tokio::test]
    async fn a_mosaic_source_is_served_and_saved_as_configured() {
        let (_, ids, saved) = resolve(indoc::indoc! {"
            cog:
              mosaics:
                ortho:
                  path: ../tests/fixtures/cog/mosaic/{quadkey}.tif
                  zoom: 16
                  maxzoom: 18
                  max_cached_files: 10
                  file_expiry: 1m
                  file_idle_timeout: 30s
        "})
        .await;
        assert_eq!(ids, ["ortho"]);
        insta::assert_json_snapshot!(saved, @r#"
        {
          "mosaics": {
            "ortho": {
              "path": "../tests/fixtures/cog/mosaic/{quadkey}.tif",
              "zoom": 16,
              "maxzoom": 18,
              "max_cached_files": 10,
              "file_expiry": "1m",
              "file_idle_timeout": "30s"
            }
          }
        }
        "#);
    }

    #[tokio::test]
    async fn an_invalid_mosaic_is_skipped_with_a_warning() {
        let (_, ids, _) = resolve(indoc::indoc! {"
            on_invalid: warn
            cog:
              mosaics:
                empty:
                  path: ../tests/fixtures/cog/mosaic/{quadkey}_missing.tif
                  zoom: 16
                bad_template:
                  path: ../tests/fixtures/cog/mosaic/{q}.tif
                  zoom: 16
                ortho:
                  path: ../tests/fixtures/cog/mosaic/{quadkey}.tif
                  zoom: 16
        "})
        .await;
        assert_eq!(ids, ["ortho"]);
    }

    #[tokio::test]
    async fn unrecognized_mosaic_keys_are_reported() {
        let mut config: Config = serde_saphyr::from_str(indoc::indoc! {"
            cog:
              mosaics:
                ortho:
                  path: ../tests/fixtures/cog/mosaic/{quadkey}.tif
                  zoom: 16
                  max_zoom: 18
        "})
        .unwrap();
        config.finalize().await.unwrap();
        insta::assert_debug_snapshot!(config.get_unrecognized_keys(), @r#"
        {
            "cog.mosaics.ortho.max_zoom",
        }
        "#);
    }

    #[tokio::test]
    async fn unrecognized_stac_keys_are_reported() {
        let mut config: Config = serde_saphyr::from_str(indoc::indoc! {"
            cog:
              stac:
                naip:
                  url: https://earth-search.aws.element84.com/v1
                  collection: naip
                  asset: image
                  minzoom: 12
                  min_zoom: 12
        "})
        .unwrap();
        config.finalize().await.unwrap();
        insta::assert_debug_snapshot!(config.get_unrecognized_keys(), @r#"
        {
            "cog.stac.naip.min_zoom",
        }
        "#);
    }

    #[tokio::test]
    async fn unrecognized_tile_index_keys_are_reported() {
        let mut config: Config = serde_saphyr::from_str(indoc::indoc! {"
            cog:
              tile_index:
                naip:
                  path: /data/naip_ca_2020.gpkg
                  minzoom: 16
                  min_zoom: 16
        "})
        .unwrap();
        config.finalize().await.unwrap();
        insta::assert_debug_snapshot!(config.get_unrecognized_keys(), @r#"
        {
            "cog.tile_index.naip.min_zoom",
        }
        "#);
    }
}
