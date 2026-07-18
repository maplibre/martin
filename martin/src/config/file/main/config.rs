use std::path::PathBuf;
use std::sync::LazyLock;

use clap::ValueEnum;
#[cfg(feature = "_tiles")]
use martin_core::tiles::BoxedSource;
use serde::{Deserialize, Serialize};
use tracing::{error, instrument, warn};

#[cfg(any(
    feature = "pmtiles",
    feature = "mbtiles",
    feature = "unstable-cog",
    feature = "geojson",
    feature = "styles",
    feature = "sprites",
    feature = "fonts",
))]
use crate::config::file::FileConfigEnum;
#[cfg(feature = "unstable-cog")]
use crate::config::file::cog::CogConfig;
#[cfg(feature = "fonts")]
use crate::config::file::fonts::FontConfig;
#[cfg(feature = "geojson")]
use crate::config::file::geojson::GeoJsonConfig;
#[cfg(feature = "mbtiles")]
use crate::config::file::mbtiles::MbtConfig;
#[cfg(feature = "pmtiles")]
use crate::config::file::pmtiles::PmtConfig;
#[cfg(feature = "postgres")]
use crate::config::file::postgres::PostgresConfig;
#[cfg(all(feature = "mlt", feature = "_tiles"))]
use crate::config::file::process::{MltProcessConfig, MvtProcessConfig};
#[cfg(feature = "sprites")]
use crate::config::file::sprites::SpriteConfig;
use crate::config::file::srv::SrvConfig;
#[cfg(feature = "styles")]
use crate::config::file::styles::StyleConfig;
use crate::config::file::{GlobalCacheConfig, UnrecognizedValues};
#[cfg(feature = "postgres")]
use crate::config::primitives::OptOneMany;
#[cfg(feature = "_tiles")]
use crate::tile_source_manager::TileSourceManager;
use crate::{MartinError, MartinResult};

/// Warnings that can occur during tile source resolution
#[derive(thiserror::Error, Debug)]
pub enum TileSourceWarning {
    #[error("Source {source_id}: {error}")]
    SourceError { source_id: String, error: String },

    #[error("Path {path}: {error}")]
    PathError { path: PathBuf, error: String },
}

#[cfg(feature = "_tiles")]
pub type ResolutionResult = MartinResult<(Vec<BoxedSource>, Vec<TileSourceWarning>)>;

pub struct ServerState {
    #[cfg(feature = "_tiles")]
    pub tile_manager: TileSourceManager,

    #[cfg(feature = "sprites")]
    pub sprites: martin_core::sprites::SpriteSources,
    #[cfg(feature = "sprites")]
    pub sprite_cache: martin_core::sprites::OptSpriteCache,

    #[cfg(feature = "fonts")]
    pub fonts: martin_core::fonts::FontSources,
    #[cfg(feature = "fonts")]
    pub font_cache: martin_core::fonts::OptFontCache,

    #[cfg(feature = "styles")]
    pub styles: martin_core::styles::StyleSources,
}

#[serde_with::skip_serializing_none]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "unstable-schemas", derive(schemars::JsonSchema))]
pub struct Config {
    /// Cache configuration
    /// Use `cache: disable` to disable all caching entirely.
    #[serde(default, skip_serializing_if = "GlobalCacheConfig::is_empty")]
    #[cfg_attr(
        feature = "unstable-schemas",
        schemars(with = "crate::config::file::GlobalCacheConfigShape")
    )]
    pub cache: GlobalCacheConfig,

    /// The policy for handling invalid sources during startup. \[default: abort\]
    ///
    /// Invalid sources are those that are missing (file not found, table doesn't exist, ...),
    /// reference columns that don't exist, and so on.
    /// Currently limited to tile sources; broader rollout is planned.
    ///
    /// Options:
    /// - `warn`: log warning messages
    /// - `abort`: log warnings as error messages, abort startup
    #[serde(default)]
    pub on_invalid: Option<OnInvalid>,

    #[serde(flatten)]
    pub srv: SrvConfig,

    /// Database configuration
    ///
    /// This can also be a list of PG configs, for example:
    /// ```yaml
    /// postgres:
    ///   - connection_string:  postgres://postgres:postgres@localhost:5432/db
    ///     default_srid: 4326
    ///   - connection_string:  postgres://postgres:postgres@another_host:5432/another_db
    ///     default_srid: 3857
    /// ```
    #[cfg(feature = "postgres")]
    #[serde(default, skip_serializing_if = "OptOneMany::is_none")]
    pub postgres: OptOneMany<PostgresConfig>,

    /// Publish `PMTiles` files from local disk or proxy to a web server
    #[cfg(feature = "pmtiles")]
    #[serde(default, skip_serializing_if = "FileConfigEnum::is_none")]
    pub pmtiles: FileConfigEnum<PmtConfig>,

    /// Publish `MBTiles` files
    #[cfg(feature = "mbtiles")]
    #[serde(default, skip_serializing_if = "FileConfigEnum::is_none")]
    pub mbtiles: FileConfigEnum<MbtConfig>,

    #[cfg(feature = "unstable-cog")]
    #[serde(default, skip_serializing_if = "FileConfigEnum::is_none")]
    pub cog: FileConfigEnum<CogConfig>,

    /// Publish `GeoJSON` files as vector tile sources
    #[cfg(feature = "geojson")]
    #[serde(default, skip_serializing_if = "FileConfigEnum::is_none")]
    pub geojson: FileConfigEnum<GeoJsonConfig>,

    /// Sprite configuration
    #[cfg(feature = "sprites")]
    #[serde(default, skip_serializing_if = "FileConfigEnum::is_none")]
    pub sprites: SpriteConfig,

    /// Publish `MapLibre` style files
    /// You can also configure us to render the styles on the server side.
    #[cfg(feature = "styles")]
    #[serde(default, skip_serializing_if = "FileConfigEnum::is_none")]
    pub styles: StyleConfig,

    /// Font configuration
    #[cfg(feature = "fonts")]
    #[serde(default, skip_serializing_if = "FileConfigEnum::is_none")]
    pub fonts: FontConfig,

    /// Encoder settings for MVT->MLT conversion (global level).
    /// Overridden by source-type or per-source `convert_to_mlt` keys.
    ///
    /// Can be either:
    /// - (default) `auto` - we choose defaults which we think work best for most users
    /// - `disabled` - no conversion
    /// - explicitely configured
    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    #[serde(default)]
    pub convert_to_mlt: Option<MltProcessConfig>,

    /// Settings for MLT->MVT conversion (global level).
    /// Overridden by source-type or per-source `convert_to_mvt` keys.
    ///
    /// Can be either:
    /// - (default) `auto` - we choose defaults which we think work best for most users
    /// - `disabled` - no conversion
    /// - explicitly configured
    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    #[serde(default)]
    pub convert_to_mvt: Option<MvtProcessConfig>,

    #[serde(flatten, skip_serializing)]
    #[cfg_attr(feature = "unstable-schemas", schemars(skip))]
    pub unrecognized: UnrecognizedValues,
}

/// Describes the action to take during startup when configuration is found to be invalid
/// but Martin could still startup in a degraded state (ie, some sources not served).
#[derive(PartialEq, Eq, Debug, Clone, Copy, Default, Serialize, Deserialize, ValueEnum)]
#[cfg_attr(feature = "unstable-schemas", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum OnInvalid {
    /// Log warning messages, abort if the error is critical
    #[serde(
        alias = "warnings",
        alias = "warning",
        alias = "continue",
        alias = "ignore"
    )]
    Warn,
    /// Log warnings as errors, abort startup
    #[default]
    Abort,
}

fn fmt_warnings(warnings: &[TileSourceWarning]) -> String {
    warnings
        .iter()
        .map(|w| format!("  - {w}"))
        .collect::<Vec<String>>()
        .join("\n")
}

impl OnInvalid {
    /// Handle warnings based on `policy`
    #[instrument(skip_all, fields(warnings.count = warnings.len()), err(Debug))]
    pub fn handle_tile_warnings(self, warnings: &[TileSourceWarning]) -> MartinResult<()> {
        if warnings.is_empty() {
            return Ok(());
        }
        match warnings {
            [warning] => match self {
                Self::Warn => warn!("Tile source resolution warning: {warning}"),
                Self::Abort => error!("Tile source resolution warning: {warning}"),
            },
            warnings => match self {
                Self::Warn => warn!("Tile source resolutions:\n{}", fmt_warnings(warnings)),
                Self::Abort => error!("Tile source resolutions:\n{}", fmt_warnings(warnings)),
            },
        }

        match self {
            Self::Abort => Err(MartinError::TileResolutionWarningsIssued),
            Self::Warn => Ok(()),
        }
    }
}

pub fn parse_base_path(path: &str) -> MartinResult<String> {
    if !path.starts_with('/') {
        return Err(MartinError::BasePathError(path.to_string()));
    }
    if let Ok(uri) = path.parse::<actix_web::http::Uri>() {
        return Ok(uri.path().trim_end_matches('/').to_string());
    }
    Err(MartinError::BasePathError(path.to_string()))
}

pub fn init_aws_lc_tls() {
    use rustls::crypto::aws_lc_rs;

    // https://github.com/rustls/rustls/issues/1877
    static INIT_TLS: LazyLock<()> = LazyLock::new(|| {
        aws_lc_rs::default_provider()
            .install_default()
            .expect("Unable to init rustls: {e:?}");
    });
    *INIT_TLS;
}

#[cfg(test)]
mod tests {
    use martin_core::CacheZoomRange;

    use super::*;
    use crate::MartinError;
    use crate::config::file::CachePolicy;
    use crate::config::test_helpers::render_finalize_failure;
    use crate::logging::LogFormat;

    #[test]
    fn non_spanned_error_renders_as_json_envelope() {
        // For errors that don't carry source location info, JSON mode still emits a JSON
        // document so downstream tools can keep parsing rather than choking on a free-form
        // log line.
        let envelope = MartinError::BasePathError("not-a-path".to_string())
            .render_diagnostic_with(LogFormat::Json);
        let parsed: serde_json::Value =
            serde_json::from_str(&envelope).unwrap_or_else(|e| panic!("not JSON: {e}\n{envelope}"));
        let msg = parsed.get("message").and_then(|m| m.as_str()).unwrap_or("");
        assert!(
            msg.contains("not-a-path"),
            "expected envelope to include the error message; got: {envelope}"
        );
    }

    #[test]
    fn parse_base_path_accepts_valid_paths() {
        assert_eq!("", parse_base_path("/").unwrap());
        assert_eq!("", parse_base_path("//").unwrap());
        assert_eq!("/foo/bar", parse_base_path("/foo/bar").unwrap());
        assert_eq!("/foo/bar", parse_base_path("/foo/bar/").unwrap());
    }

    #[test]
    fn parse_base_path_rejects_invalid_paths() {
        parse_base_path("").unwrap_err();
        parse_base_path("foo/bar").unwrap_err();
    }

    #[tokio::test]
    async fn finalize_base_path_must_start_with_slash() {
        insta::assert_snapshot!(
            render_finalize_failure(indoc::indoc! {"
                pmtiles: /tmp
                base_path: not-a-path
            "}).await,
            @"Base path must be a valid URL path, and must begin with a '/' symbol, but is 'not-a-path'"
        );
    }

    #[tokio::test]
    async fn finalize_route_prefix_must_start_with_slash() {
        insta::assert_snapshot!(
            render_finalize_failure(indoc::indoc! {"
                pmtiles: /tmp
                route_prefix: oops
            "}).await,
            @"Base path must be a valid URL path, and must begin with a '/' symbol, but is 'oops'"
        );
    }

    #[test]
    fn cache_disable_per_source() {
        let policy: CachePolicy = serde_saphyr::from_str("disable").unwrap();
        assert_eq!(policy, CachePolicy::disabled());
        for zoom in 0..=u8::MAX {
            assert!(
                !policy.zoom().contains(zoom),
                "A disabled policy should never match any zoom level"
            );
        }
    }

    #[test]
    fn cache_disable_per_source_ignores_global_defaults() {
        // Per-source disable is not overridden by global defaults
        let disabled = CachePolicy::disabled();
        let defaults = CachePolicy::new(CacheZoomRange::new(Some(0), Some(20)));
        let merged = disabled.or(defaults);
        for zoom in 0..=u8::MAX {
            assert!(!merged.zoom().contains(zoom));
        }
    }

    #[test]
    fn cache_disable_global_can_be_overridden_per_source() {
        // Per-source config re-enables caching despite global disable
        let source = CachePolicy::new(CacheZoomRange::new(Some(0), Some(10)));
        let global_disabled = CachePolicy::disabled();
        let merged = source.or(global_disabled);
        assert!(merged.zoom().contains(0));
        assert!(merged.zoom().contains(5));
        assert!(merged.zoom().contains(10));
        assert!(!merged.zoom().contains(11));
    }
}
