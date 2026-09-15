use std::path::{Path, PathBuf};

#[cfg(feature = "fonts")]
use martin_core::fonts::FontError;
#[cfg(feature = "sprites")]
use martin_core::sprites::SpriteError;
#[cfg(feature = "postgres")]
use martin_core::tiles::postgres::PostgresError;
use miette::{Diagnostic, LabeledSpan, NamedSource, SourceCode};

#[cfg(all(feature = "contour", feature = "_tiles"))]
use crate::config::file::contour::ContourRangeError;
#[cfg(all(feature = "hillshade", feature = "_tiles"))]
use crate::config::file::hillshade::HillshadeRangeError;

pub type ConfigFileResult<T> = Result<T, ConfigFileError>;

#[derive(thiserror::Error, Debug)]
pub enum ConfigFileError {
    #[error("IO error {0}: {1}")]
    IoError(#[source] std::io::Error, PathBuf),

    #[error("Unable to load config file {1}: {0}")]
    ConfigLoadError(#[source] std::io::Error, PathBuf),

    #[error("Unable to parse YAML in config file {}: {}", .0.named_source.name(), .0.error)]
    YamlParseError(Box<YamlParseDetails>),

    #[error("Unable to write config file {1}: {0}")]
    ConfigWriteError(#[source] std::io::Error, PathBuf),

    #[error(
        "No tile sources found. Set sources by giving a database connection string on command line, env variable, or a config file."
    )]
    NoSources,
    #[error("Source path is not a file: {0}")]
    InvalidFilePath(PathBuf),

    #[error("Error {0} while parsing URL {1}")]
    InvalidSourceUrl(#[source] url::ParseError, String),

    #[error("Could not parse source path {0} as a URL")]
    PathNotConvertibleToUrl(PathBuf),

    #[error("Source {0} uses bad file {1}")]
    InvalidSourceFilePath(String, PathBuf),

    #[cfg(feature = "passthrough")]
    #[error(
        "Passthrough source {source_id} has an unknown tile format {tile_format:?}; expected one of pbf/mvt, mlt, png, jpg, webp, json, gif, avif"
    )]
    InvalidPassthroughFormat {
        source_id: String,
        tile_format: String,
    },

    #[error("At least one 'origin' must be specified in the 'cors' configuration")]
    CorsNoOriginsConfigured,

    #[error("Base path must be a valid URL path, and must begin with a '/' symbol, but is '{0}'")]
    InvalidBasePath(String),

    #[error("warnings issued during tile source resolution")]
    TileResolutionWarningsIssued,

    #[cfg(all(feature = "hillshade", feature = "_tiles"))]
    #[error("Source {source_id} has an invalid hillshade configuration: {source}")]
    InvalidHillshade {
        source_id: String,
        #[source]
        source: Box<HillshadeRangeError>,
    },

    #[cfg(all(feature = "contour", feature = "_tiles"))]
    #[error("Source {source_id} has an invalid contour configuration: {source}")]
    InvalidContour {
        source_id: String,
        #[source]
        source: Box<ContourRangeError>,
    },

    #[cfg(feature = "styles")]
    #[error("Walk directory error {0}: {1}")]
    DirectoryWalking(#[source] walkdir::Error, PathBuf),

    #[cfg(feature = "postgres")]
    #[error("A postgres connection string must be provided")]
    PostgresConnectionStringMissing,

    #[cfg(feature = "postgres")]
    #[error("Failed to create postgres pool: {0}")]
    PostgresPoolCreationFailed(#[source] PostgresError),

    #[cfg(feature = "fonts")]
    #[error("Failed to load fonts from {1}: {0}")]
    FontResolutionFailed(#[source] FontError, PathBuf),

    #[cfg(feature = "fonts")]
    #[error("Failed to configure font alias: {0}")]
    FontAliasResolutionFailed(#[source] FontError),

    #[cfg(feature = "sprites")]
    #[error("Failed to configure sprite alias: {0}")]
    SpriteAliasResolutionFailed(#[source] SpriteError),

    #[cfg(feature = "_tiles")]
    #[error("Failed to configure tile source alias: {0}")]
    TileAliasResolutionFailed(#[source] crate::source::TileAliasError),

    #[cfg(any(feature = "pmtiles", feature = "unstable-cog"))]
    #[error("Failed to parse object store URL of {1}: {0}")]
    ObjectStoreUrlParsing(object_store::Error, String),

    #[cfg(any(feature = "pmtiles", feature = "unstable-cog"))]
    #[error("Failed to list objects under {1}: {0}")]
    ObjectStoreList(object_store::Error, String),

    #[cfg(all(feature = "rendering", target_os = "linux"))]
    #[error("Failed to start style render pool: {0}")]
    RendererPoolSpawnFailed(#[source] std::io::Error),

    #[cfg(feature = "_tiles")]
    #[error("Tile grid {0} would redefine the built-in grid of that name, pick another name")]
    TileGridRedefinesBuiltIn(String),

    #[cfg(feature = "_tiles")]
    #[error(transparent)]
    InvalidTileGrid(#[from] martin_tile_utils::TileGridError),

    #[cfg(feature = "_tiles")]
    #[error(
        "{what} names a tile grid, but only PostgreSQL, MBTiles and PMTiles sources can be served on one"
    )]
    TileGridNotSupported { what: String },

    #[cfg(feature = "_tiles")]
    #[error("{what} refers to tile grid {grid}, which is not configured. Known grids: {known}")]
    UnknownTileGrid {
        what: String,
        grid: String,
        known: String,
    },

    #[cfg(feature = "postgres")]
    #[error("Tile grid {grid} uses {crs}, whose code is not an integer")]
    TileGridCrsCodeNotNumeric { grid: String, crs: String },

    #[cfg(feature = "postgres")]
    #[error("Tile grid {grid} uses {crs}, which has no spatial_ref_sys row in the database")]
    TileGridCrsNotInDatabase { grid: String, crs: String },

    #[cfg(feature = "postgres")]
    #[error("Failed to resolve tile grid {1} against the database: {0}")]
    TileGridResolution(Box<PostgresError>, String),
}

/// Boxed payload for [`ConfigFileError::YamlParseError`].
#[derive(Debug)]
pub struct YamlParseDetails {
    pub(crate) error: serde_saphyr::Error,
    pub(crate) named_source: NamedSource<String>,
}

impl ConfigFileError {
    /// Construct a YAML parse error with the originating source text and file path.
    ///
    /// The source text is retained so miette diagnostics can render the offending snippet.
    #[must_use]
    pub fn yaml_parse(error: serde_saphyr::Error, source_text: String, file_path: &Path) -> Self {
        Self::YamlParseError(Box::new(YamlParseDetails {
            error,
            named_source: NamedSource::new(file_path.display().to_string(), source_text),
        }))
    }

    /// Render this error as a [`miette::Report`] for graphical display, when applicable.
    #[must_use]
    pub fn to_miette_report(&self) -> Option<miette::Report> {
        let Self::YamlParseError(details) = self else {
            return None;
        };
        let inner = serde_saphyr::miette::to_miette_report(
            &details.error,
            details.named_source.inner(),
            details.named_source.name(),
        );
        let kind = YamlReportKind::for_error(&details.error);
        Some(miette::Report::new(YamlParseReport { inner, kind }))
    }
}

#[derive(Clone, Copy, Debug)]
enum YamlReportKind {
    Substitution,
    Yaml,
}

impl YamlReportKind {
    #[expect(
        clippy::wildcard_enum_match_arm,
        reason = "serde_saphyr::Error is #[non_exhaustive] with dozens of variants that all render as plain YAML errors"
    )]
    fn for_error(err: &serde_saphyr::Error) -> Self {
        use serde_saphyr::Error::{
            InvalidPropertyName, PropertyRequiredButEmpty, PropertyRequiredButUnset,
            UnresolvedProperty, WithSnippet,
        };

        match err {
            UnresolvedProperty { .. }
            | InvalidPropertyName { .. }
            | PropertyRequiredButUnset { .. }
            | PropertyRequiredButEmpty { .. } => Self::Substitution,
            WithSnippet { error, .. }
                if matches!(
                    error.as_ref(),
                    UnresolvedProperty { .. }
                        | InvalidPropertyName { .. }
                        | PropertyRequiredButUnset { .. }
                        | PropertyRequiredButEmpty { .. }
                ) =>
            {
                Self::Substitution
            }
            _ => Self::Yaml,
        }
    }

    const fn code(self) -> &'static str {
        match self {
            Self::Substitution => "martin::config::substitution",
            Self::Yaml => "martin::config::yaml",
        }
    }

    const fn help(self) -> &'static str {
        match self {
            Self::Substitution => {
                "Make sure every ${VAR} reference resolves to an environment variable, or supply a default with `${VAR:-fallback}`."
            }
            Self::Yaml => {
                "Check the highlighted token in your YAML. The error usually indicates a mismatched type or an unexpected shape."
            }
        }
    }
}

#[derive(Debug)]
struct YamlParseReport {
    inner: miette::Report,
    kind: YamlReportKind,
}

impl YamlParseReport {
    fn inner_diag(&self) -> &(dyn Diagnostic + 'static) {
        <miette::Report as AsRef<dyn Diagnostic>>::as_ref(&self.inner)
    }
}

impl std::fmt::Display for YamlParseReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(self.inner_diag(), f)
    }
}

impl std::error::Error for YamlParseReport {}

impl Diagnostic for YamlParseReport {
    fn code<'a>(&'a self) -> Option<Box<dyn std::fmt::Display + 'a>> {
        Some(Box::new(self.kind.code()))
    }

    fn help<'a>(&'a self) -> Option<Box<dyn std::fmt::Display + 'a>> {
        Some(Box::new(self.kind.help()))
    }

    fn url<'a>(&'a self) -> Option<Box<dyn std::fmt::Display + 'a>> {
        Some(Box::new("https://maplibre.org/martin/config-file/"))
    }

    fn severity(&self) -> Option<miette::Severity> {
        self.inner_diag().severity()
    }

    fn source_code(&self) -> Option<&dyn SourceCode> {
        self.inner_diag().source_code()
    }

    fn labels(&self) -> Option<Box<dyn Iterator<Item = LabeledSpan> + '_>> {
        self.inner_diag().labels()
    }

    fn related(&self) -> Option<Box<dyn Iterator<Item = &dyn Diagnostic> + '_>> {
        self.inner_diag().related()
    }

    fn diagnostic_source(&self) -> Option<&dyn Diagnostic> {
        self.inner_diag().diagnostic_source()
    }
}

impl Diagnostic for ConfigFileError {
    fn code<'a>(&'a self) -> Option<Box<dyn std::fmt::Display + 'a>> {
        let code: &'static str = match self {
            Self::IoError(..) => "martin::config::io",
            Self::ConfigLoadError(..) => "martin::config::io::load",
            Self::ConfigWriteError(..) => "martin::config::io::write",
            Self::YamlParseError { .. } => "martin::config::yaml",
            Self::NoSources => "martin::config::no_sources",
            Self::InvalidFilePath(_) => "martin::config::invalid_file_path",
            Self::InvalidSourceUrl(..) => "martin::config::invalid_source_url",
            Self::PathNotConvertibleToUrl(_) => "martin::config::path_not_url",
            Self::InvalidSourceFilePath(..) => "martin::config::invalid_source_file_path",
            #[cfg(feature = "passthrough")]
            Self::InvalidPassthroughFormat { .. } => "martin::config::passthrough::invalid_format",
            Self::CorsNoOriginsConfigured => "martin::config::cors::no_origins",
            Self::InvalidBasePath(_) => "martin::config::invalid_base_path",
            Self::TileResolutionWarningsIssued => "martin::config::tile_resolution_warnings",
            #[cfg(all(feature = "hillshade", feature = "_tiles"))]
            Self::InvalidHillshade { .. } => "martin::config::hillshade::invalid",
            #[cfg(all(feature = "contour", feature = "_tiles"))]
            Self::InvalidContour { .. } => "martin::config::contour::invalid",
            #[cfg(feature = "styles")]
            Self::DirectoryWalking(..) => "martin::config::styles::walk",
            #[cfg(feature = "postgres")]
            Self::PostgresConnectionStringMissing => "martin::config::postgres::connection_string",
            #[cfg(feature = "postgres")]
            Self::PostgresPoolCreationFailed(_) => "martin::config::postgres::pool_creation",
            #[cfg(feature = "fonts")]
            Self::FontResolutionFailed(..) => "martin::config::fonts::resolution",
            #[cfg(feature = "fonts")]
            Self::FontAliasResolutionFailed(_) => "martin::config::fonts::alias",
            #[cfg(feature = "sprites")]
            Self::SpriteAliasResolutionFailed(_) => "martin::config::sprites::alias",
            #[cfg(feature = "_tiles")]
            Self::TileAliasResolutionFailed(_) => "martin::config::aliases",
            #[cfg(any(feature = "pmtiles", feature = "unstable-cog"))]
            Self::ObjectStoreUrlParsing(..) => "martin::config::pmtiles::object_store_url",
            #[cfg(any(feature = "pmtiles", feature = "unstable-cog"))]
            Self::ObjectStoreList(..) => "martin::config::pmtiles::object_store_list",
            #[cfg(all(feature = "rendering", target_os = "linux"))]
            Self::RendererPoolSpawnFailed(_) => "martin::config::styles::render_pool_spawn",
            #[cfg(feature = "_tiles")]
            Self::TileGridRedefinesBuiltIn(_) => "martin::config::tile_grids::redefines_built_in",
            #[cfg(feature = "_tiles")]
            Self::InvalidTileGrid(_) => "martin::config::tile_grids::invalid",
            #[cfg(feature = "_tiles")]
            Self::UnknownTileGrid { .. } => "martin::config::tile_grids::unknown",
            #[cfg(feature = "_tiles")]
            Self::TileGridNotSupported { .. } => "martin::config::tile_grids::unsupported_kind",
            #[cfg(feature = "postgres")]
            Self::TileGridCrsCodeNotNumeric { .. } => "martin::config::tile_grids::crs_code",
            #[cfg(feature = "postgres")]
            Self::TileGridCrsNotInDatabase { .. } => {
                "martin::config::tile_grids::crs_not_in_database"
            }
            #[cfg(feature = "postgres")]
            Self::TileGridResolution(..) => "martin::config::tile_grids::resolution",
        };
        Some(Box::new(code))
    }

    fn help<'a>(&'a self) -> Option<Box<dyn std::fmt::Display + 'a>> {
        let help: &'static str = match self {
            Self::NoSources => {
                "Provide tile sources via --connection, environment variables (e.g. DATABASE_URL), or a config file passed with --config."
            }
            Self::CorsNoOriginsConfigured => {
                "Either set `cors: true` (allow all origins) or provide at least one entry in `origin` under the cors block."
            }
            Self::YamlParseError { .. } => {
                "Check the highlighted token in your YAML. The error usually indicates a mismatched type or an unexpected shape."
            }
            Self::IoError(..)
            | Self::ConfigLoadError(..)
            | Self::ConfigWriteError(..)
            | Self::InvalidFilePath(_)
            | Self::InvalidSourceUrl(..)
            | Self::PathNotConvertibleToUrl(_)
            | Self::InvalidSourceFilePath(..)
            | Self::InvalidBasePath(_)
            | Self::TileResolutionWarningsIssued => return None,
            #[cfg(all(feature = "hillshade", feature = "_tiles"))]
            Self::InvalidHillshade { .. } => {
                "Check the `hillshade` block of the named source: every parameter must lie inside the range given above."
            }
            #[cfg(all(feature = "contour", feature = "_tiles"))]
            Self::InvalidContour { .. } => {
                "Check the `contour` block of the named source: every parameter must lie inside the range given above."
            }
            #[cfg(feature = "passthrough")]
            Self::InvalidPassthroughFormat { .. } => return None,
            #[cfg(feature = "styles")]
            Self::DirectoryWalking(..) => return None,
            #[cfg(feature = "postgres")]
            Self::PostgresConnectionStringMissing | Self::PostgresPoolCreationFailed(_) => {
                return None;
            }
            #[cfg(feature = "fonts")]
            Self::FontResolutionFailed(..) => return None,
            #[cfg(feature = "fonts")]
            Self::FontAliasResolutionFailed(_) => {
                "Check the `fonts.aliases` block: every alias must list at least one discovered font by its catalog name, and aliases cannot reference other aliases."
            }
            #[cfg(feature = "sprites")]
            Self::SpriteAliasResolutionFailed(_) => {
                "Check the `sprites.aliases` block: every alias must list at least one configured sprite source by its id, and aliases cannot reference other aliases."
            }
            #[cfg(feature = "_tiles")]
            Self::TileAliasResolutionFailed(_) => {
                "Check the `aliases` block: every alias must list at least one configured tile source by its id, and aliases cannot reference other aliases."
            }
            #[cfg(any(feature = "pmtiles", feature = "unstable-cog"))]
            Self::ObjectStoreUrlParsing(..) | Self::ObjectStoreList(..) => return None,
            #[cfg(all(feature = "rendering", target_os = "linux"))]
            Self::RendererPoolSpawnFailed(_) => return None,
            #[cfg(feature = "_tiles")]
            Self::TileGridRedefinesBuiltIn(_) | Self::InvalidTileGrid(_) => {
                "Check the `tile_grids` block: each grid needs a `crs` like `EPSG:2193`, an `origin` `[x, y]` and a positive `extent_at_zoom0`, all in CRS units."
            }
            #[cfg(feature = "_tiles")]
            Self::UnknownTileGrid { .. } => {
                "A `tile_grid` must name a grid defined under the top-level `tile_grids`, or a built-in one."
            }
            #[cfg(feature = "_tiles")]
            Self::TileGridNotSupported { .. } => {
                "COG, GeoJSON and DuckDB sources produce Web Mercator tiles. Remove the `tile_grid` from this source."
            }
            #[cfg(feature = "postgres")]
            Self::TileGridCrsCodeNotNumeric { .. } => {
                "Write the CRS as `AUTHORITY:CODE` with an integer code, for example `EPSG:2193` or `IAU_2015:49900`."
            }
            #[cfg(feature = "postgres")]
            Self::TileGridCrsNotInDatabase { .. } => {
                "PostGIS only knows the coordinate reference systems in its `spatial_ref_sys` table. Insert a row whose `auth_name` and `auth_srid` are the two halves of the CRS identifier, with its `proj4text` or `srtext`, or use an EPSG code."
            }
            #[cfg(feature = "postgres")]
            Self::TileGridResolution(..) => return None,
        };
        Some(Box::new(help))
    }

    fn url<'a>(&'a self) -> Option<Box<dyn std::fmt::Display + 'a>> {
        Some(Box::new("https://maplibre.org/martin/config-file/"))
    }

    // Carets and labels come from `to_miette_report`.
    // Surface the file here so direct rendering still shows it.
    fn source_code(&self) -> Option<&dyn SourceCode> {
        let Self::YamlParseError(details) = self else {
            return None;
        };
        Some(&details.named_source)
    }

    fn labels(&self) -> Option<Box<dyn Iterator<Item = LabeledSpan> + '_>> {
        None
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    #[cfg(any(feature = "pmtiles", feature = "unstable-cog"))]
    fn object_store_error() -> object_store::Error {
        object_store::Error::NotImplemented {
            operation: "list".to_owned(),
            implementer: "test".to_owned(),
        }
    }

    fn config_yaml() -> PathBuf {
        PathBuf::from("config.yaml")
    }

    fn describe(err: &ConfigFileError) -> String {
        let some_or_none = |is_some: bool| if is_some { "some" } else { "none" };
        format!(
            "message: {err}\ncode: {}\nhelp: {}\nurl: {}\nlabels: {}\nsource_code: {}\nmiette_report: {}",
            err.code().expect("a code"),
            err.help()
                .map_or_else(|| "none".to_owned(), |h| h.to_string()),
            err.url().expect("a url"),
            some_or_none(err.labels().is_some()),
            some_or_none(err.source_code().is_some()),
            some_or_none(err.to_miette_report().is_some()),
        )
    }

    #[test]
    fn io_error() {
        let err = ConfigFileError::IoError(std::io::Error::other("boom"), config_yaml());
        insta::assert_snapshot!(describe(&err), @"
        message: IO error boom: config.yaml
        code: martin::config::io
        help: none
        url: https://maplibre.org/martin/config-file/
        labels: none
        source_code: none
        miette_report: none
        ");
    }

    #[test]
    fn config_load_error() {
        let err = ConfigFileError::ConfigLoadError(std::io::Error::other("boom"), config_yaml());
        insta::assert_snapshot!(describe(&err), @"
        message: Unable to load config file config.yaml: boom
        code: martin::config::io::load
        help: none
        url: https://maplibre.org/martin/config-file/
        labels: none
        source_code: none
        miette_report: none
        ");
    }

    #[test]
    fn config_write_error() {
        let err = ConfigFileError::ConfigWriteError(std::io::Error::other("boom"), config_yaml());
        insta::assert_snapshot!(describe(&err), @"
        message: Unable to write config file config.yaml: boom
        code: martin::config::io::write
        help: none
        url: https://maplibre.org/martin/config-file/
        labels: none
        source_code: none
        miette_report: none
        ");
    }

    #[test]
    fn no_sources() {
        let err = ConfigFileError::NoSources;
        insta::assert_snapshot!(describe(&err), @"
        message: No tile sources found. Set sources by giving a database connection string on command line, env variable, or a config file.
        code: martin::config::no_sources
        help: Provide tile sources via --connection, environment variables (e.g. DATABASE_URL), or a config file passed with --config.
        url: https://maplibre.org/martin/config-file/
        labels: none
        source_code: none
        miette_report: none
        ");
    }

    #[test]
    fn invalid_file_path() {
        let err = ConfigFileError::InvalidFilePath(config_yaml());
        insta::assert_snapshot!(describe(&err), @"
        message: Source path is not a file: config.yaml
        code: martin::config::invalid_file_path
        help: none
        url: https://maplibre.org/martin/config-file/
        labels: none
        source_code: none
        miette_report: none
        ");
    }

    #[test]
    fn invalid_source_url() {
        let err =
            ConfigFileError::InvalidSourceUrl(url::ParseError::EmptyHost, "http://".to_owned());
        insta::assert_snapshot!(describe(&err), @"
        message: Error empty host while parsing URL http://
        code: martin::config::invalid_source_url
        help: none
        url: https://maplibre.org/martin/config-file/
        labels: none
        source_code: none
        miette_report: none
        ");
    }

    #[test]
    fn path_not_convertible_to_url() {
        let err = ConfigFileError::PathNotConvertibleToUrl(config_yaml());
        insta::assert_snapshot!(describe(&err), @"
        message: Could not parse source path config.yaml as a URL
        code: martin::config::path_not_url
        help: none
        url: https://maplibre.org/martin/config-file/
        labels: none
        source_code: none
        miette_report: none
        ");
    }

    #[test]
    fn invalid_source_file_path() {
        let err = ConfigFileError::InvalidSourceFilePath("src".to_owned(), config_yaml());
        insta::assert_snapshot!(describe(&err), @"
        message: Source src uses bad file config.yaml
        code: martin::config::invalid_source_file_path
        help: none
        url: https://maplibre.org/martin/config-file/
        labels: none
        source_code: none
        miette_report: none
        ");
    }

    #[cfg(feature = "passthrough")]
    #[test]
    fn invalid_passthrough_format() {
        let err = ConfigFileError::InvalidPassthroughFormat {
            source_id: "src".to_owned(),
            tile_format: "tiff".to_owned(),
        };
        insta::assert_snapshot!(describe(&err), @r#"
        message: Passthrough source src has an unknown tile format "tiff"; expected one of pbf/mvt, mlt, png, jpg, webp, json, gif, avif
        code: martin::config::passthrough::invalid_format
        help: none
        url: https://maplibre.org/martin/config-file/
        labels: none
        source_code: none
        miette_report: none
        "#);
    }

    #[test]
    fn cors_no_origins_configured() {
        let err = ConfigFileError::CorsNoOriginsConfigured;
        insta::assert_snapshot!(describe(&err), @"
        message: At least one 'origin' must be specified in the 'cors' configuration
        code: martin::config::cors::no_origins
        help: Either set `cors: true` (allow all origins) or provide at least one entry in `origin` under the cors block.
        url: https://maplibre.org/martin/config-file/
        labels: none
        source_code: none
        miette_report: none
        ");
    }

    #[test]
    fn invalid_base_path() {
        let err = ConfigFileError::InvalidBasePath("no-slash".to_owned());
        insta::assert_snapshot!(describe(&err), @"
        message: Base path must be a valid URL path, and must begin with a '/' symbol, but is 'no-slash'
        code: martin::config::invalid_base_path
        help: none
        url: https://maplibre.org/martin/config-file/
        labels: none
        source_code: none
        miette_report: none
        ");
    }

    #[test]
    fn tile_resolution_warnings_issued() {
        let err = ConfigFileError::TileResolutionWarningsIssued;
        insta::assert_snapshot!(describe(&err), @"
        message: warnings issued during tile source resolution
        code: martin::config::tile_resolution_warnings
        help: none
        url: https://maplibre.org/martin/config-file/
        labels: none
        source_code: none
        miette_report: none
        ");
    }

    #[cfg(all(feature = "hillshade", feature = "_tiles"))]
    #[test]
    fn invalid_hillshade() {
        let err = ConfigFileError::InvalidHillshade {
            source_id: "src".to_owned(),
            source: Box::new(HillshadeRangeError {
                name: "azimuth".to_owned(),
                value: "400".to_owned(),
                low: "0".to_owned(),
                high: "360".to_owned(),
            }),
        };
        insta::assert_snapshot!(describe(&err), @"
        message: Source src has an invalid hillshade configuration: Hillshade parameter azimuth must be between `0` and `360`, but was `400`
        code: martin::config::hillshade::invalid
        help: Check the `hillshade` block of the named source: every parameter must lie inside the range given above.
        url: https://maplibre.org/martin/config-file/
        labels: none
        source_code: none
        miette_report: none
        ");
    }

    #[cfg(all(feature = "contour", feature = "_tiles"))]
    #[test]
    fn invalid_contour() {
        let err = ConfigFileError::InvalidContour {
            source_id: "src".to_owned(),
            source: Box::new(ContourRangeError {
                name: "interval".to_owned(),
                value: "-1".to_owned(),
                low: "0".to_owned(),
                high: "10000".to_owned(),
            }),
        };
        insta::assert_snapshot!(describe(&err), @"
        message: Source src has an invalid contour configuration: Contour parameter interval must be between `0` and `10000`, but was `-1`
        code: martin::config::contour::invalid
        help: Check the `contour` block of the named source: every parameter must lie inside the range given above.
        url: https://maplibre.org/martin/config-file/
        labels: none
        source_code: none
        miette_report: none
        ");
    }

    #[cfg(feature = "styles")]
    #[test]
    fn directory_walking() {
        let walk_err = walkdir::WalkDir::new("/definitely/not/here")
            .into_iter()
            .next()
            .expect("a missing root yields an entry")
            .expect_err("a missing root yields an error");
        let err = ConfigFileError::DirectoryWalking(walk_err, config_yaml());
        insta::assert_snapshot!(describe(&err), @"
        message: Walk directory error IO error for operation on /definitely/not/here: No such file or directory (os error 2): config.yaml
        code: martin::config::styles::walk
        help: none
        url: https://maplibre.org/martin/config-file/
        labels: none
        source_code: none
        miette_report: none
        ");
    }

    #[cfg(feature = "postgres")]
    #[test]
    fn postgres_connection_string_missing() {
        let err = ConfigFileError::PostgresConnectionStringMissing;
        insta::assert_snapshot!(describe(&err), @"
        message: A postgres connection string must be provided
        code: martin::config::postgres::connection_string
        help: none
        url: https://maplibre.org/martin/config-file/
        labels: none
        source_code: none
        miette_report: none
        ");
    }

    #[cfg(feature = "postgres")]
    #[test]
    fn postgres_pool_creation_failed() {
        let err = ConfigFileError::PostgresPoolCreationFailed(PostgresError::InvalidFilter(
            "x".to_owned(),
            "y".to_owned(),
        ));
        insta::assert_snapshot!(describe(&err), @"
        message: Failed to create postgres pool: Filter 'x' is not valid CQL2: y
        code: martin::config::postgres::pool_creation
        help: none
        url: https://maplibre.org/martin/config-file/
        labels: none
        source_code: none
        miette_report: none
        ");
    }

    #[cfg(feature = "fonts")]
    #[test]
    fn font_resolution_failed() {
        let err = ConfigFileError::FontResolutionFailed(
            FontError::FontNotFound("Roboto".to_owned()),
            config_yaml(),
        );
        insta::assert_snapshot!(describe(&err), @"
        message: Failed to load fonts from config.yaml: Font Roboto not found
        code: martin::config::fonts::resolution
        help: none
        url: https://maplibre.org/martin/config-file/
        labels: none
        source_code: none
        miette_report: none
        ");
    }

    #[cfg(feature = "fonts")]
    #[test]
    fn font_alias_resolution_failed() {
        let err = ConfigFileError::FontAliasResolutionFailed(FontError::FontNotFound(
            "Roboto".to_owned(),
        ));
        insta::assert_snapshot!(describe(&err), @"
        message: Failed to configure font alias: Font Roboto not found
        code: martin::config::fonts::alias
        help: Check the `fonts.aliases` block: every alias must list at least one discovered font by its catalog name, and aliases cannot reference other aliases.
        url: https://maplibre.org/martin/config-file/
        labels: none
        source_code: none
        miette_report: none
        ");
    }

    #[cfg(feature = "sprites")]
    #[test]
    fn sprite_alias_resolution_failed() {
        let err = ConfigFileError::SpriteAliasResolutionFailed(SpriteError::SpriteNotFound(
            "icons".to_owned(),
        ));
        insta::assert_snapshot!(describe(&err), @"
        message: Failed to configure sprite alias: Sprite icons not found
        code: martin::config::sprites::alias
        help: Check the `sprites.aliases` block: every alias must list at least one configured sprite source by its id, and aliases cannot reference other aliases.
        url: https://maplibre.org/martin/config-file/
        labels: none
        source_code: none
        miette_report: none
        ");
    }

    #[cfg(feature = "_tiles")]
    #[test]
    fn tile_alias_resolution_failed() {
        let err = ConfigFileError::TileAliasResolutionFailed(
            crate::source::TileAliasError::EmptyAlias("all".to_owned()),
        );
        insta::assert_snapshot!(describe(&err), @r#"
        message: Failed to configure tile source alias: Tile source alias "all" does not reference any tile sources
        code: martin::config::aliases
        help: Check the `aliases` block: every alias must list at least one configured tile source by its id, and aliases cannot reference other aliases.
        url: https://maplibre.org/martin/config-file/
        labels: none
        source_code: none
        miette_report: none
        "#);
    }

    #[cfg(any(feature = "pmtiles", feature = "unstable-cog"))]
    #[test]
    fn object_store_url_parsing() {
        let err =
            ConfigFileError::ObjectStoreUrlParsing(object_store_error(), "s3://bucket".to_owned());
        insta::assert_snapshot!(describe(&err), @"
        message: Failed to parse object store URL of s3://bucket: Operation list not yet implemented by test.
        code: martin::config::pmtiles::object_store_url
        help: none
        url: https://maplibre.org/martin/config-file/
        labels: none
        source_code: none
        miette_report: none
        ");
    }

    #[cfg(any(feature = "pmtiles", feature = "unstable-cog"))]
    #[test]
    fn object_store_list() {
        let err = ConfigFileError::ObjectStoreList(object_store_error(), "s3://bucket".to_owned());
        insta::assert_snapshot!(describe(&err), @"
        message: Failed to list objects under s3://bucket: Operation list not yet implemented by test.
        code: martin::config::pmtiles::object_store_list
        help: none
        url: https://maplibre.org/martin/config-file/
        labels: none
        source_code: none
        miette_report: none
        ");
    }

    #[cfg(all(feature = "rendering", target_os = "linux"))]
    #[test]
    fn renderer_pool_spawn_failed() {
        let err = ConfigFileError::RendererPoolSpawnFailed(std::io::Error::other("boom"));
        insta::assert_snapshot!(describe(&err), @"
        message: Failed to start style render pool: boom
        code: martin::config::styles::render_pool_spawn
        help: none
        url: https://maplibre.org/martin/config-file/
        labels: none
        source_code: none
        miette_report: none
        ");
    }

    fn yaml_error(yaml: &str, with_snippet: bool) -> ConfigFileError {
        let options = serde_saphyr::options! {
            with_snippet: with_snippet,
            property_syntax: serde_saphyr::options::PropertySyntax::BracedOrBare,
        }
        .with_properties(HashMap::new());
        let err = serde_saphyr::from_str_with_options::<HashMap<String, String>>(yaml, options)
            .expect_err("the yaml is invalid");
        ConfigFileError::yaml_parse(err, yaml.to_owned(), Path::new("config.yaml"))
    }

    #[test]
    fn yaml_parse_error_carries_the_source() {
        let err = yaml_error("key: [unterminated", false);
        assert_eq!(
            err.code().expect("a code").to_string(),
            "martin::config::yaml"
        );
        assert!(err.help().is_some());
        assert!(err.source_code().is_some());
        assert!(
            err.to_string()
                .starts_with("Unable to parse YAML in config file config.yaml")
        );

        let report = err
            .to_miette_report()
            .expect("yaml errors render as reports");
        let diag: &dyn Diagnostic = report.as_ref();
        assert_eq!(
            diag.code().expect("a code").to_string(),
            "martin::config::yaml"
        );
        assert!(
            diag.help()
                .expect("help")
                .to_string()
                .contains("highlighted token")
        );
        assert_eq!(
            diag.url().expect("a url").to_string(),
            "https://maplibre.org/martin/config-file/"
        );
        assert!(diag.source_code().is_some());
        assert!(diag.labels().is_some());
        assert!(diag.related().is_none());
        assert!(diag.diagnostic_source().is_none());
        assert!(diag.severity().is_none());
        assert!(!report.to_string().is_empty());
    }

    #[test]
    fn unresolved_substitution_is_reported_as_such_with_and_without_snippet() {
        for with_snippet in [false, true] {
            let err = yaml_error("key: ${MARTIN_TEST_UNSET_VARIABLE}", with_snippet);
            let report = err.to_miette_report().expect("a report");
            let diag: &dyn Diagnostic = report.as_ref();
            assert_eq!(
                diag.code().expect("a code").to_string(),
                "martin::config::substitution",
                "with_snippet={with_snippet}"
            );
            assert!(diag.help().expect("help").to_string().contains("${VAR}"));
        }
    }
}
