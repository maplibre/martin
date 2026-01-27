use std::ffi::OsStr;
use std::fs::File;
use std::io::prelude::*;
use std::path::Path;
use std::sync::LazyLock;

use clap::ValueEnum;
#[cfg(feature = "_tiles")]
use futures::future::{BoxFuture, try_join_all};
#[cfg(feature = "_tiles")]
use martin_core::config::IdResolver;
#[cfg(feature = "postgres")]
use martin_core::config::OptOneMany;
#[cfg(feature = "_tiles")]
use martin_core::tiles::OptTileCache;
#[cfg(feature = "pmtiles")]
use martin_core::tiles::pmtiles::PmtCache;
use serde::{Deserialize, Serialize};
use subst::VariableMap;
use tracing::{error, info, warn};

#[cfg(any(
    feature = "pmtiles",
    feature = "mbtiles",
    feature = "unstable-cog",
    feature = "styles",
    feature = "sprites",
    feature = "fonts",
))]
use crate::config::file::FileConfigEnum;
#[cfg(any(feature = "_tiles", feature = "sprites", feature = "fonts"))]
use crate::config::file::cache::CacheConfig;
use crate::config::file::{
    ConfigFileError, ConfigFileResult, ConfigurationLivecycleHooks, UnrecognizedKeys,
    UnrecognizedValues, copy_unrecognized_keys_from_config,
};
#[cfg(feature = "_tiles")]
use crate::source::TileSources;
#[cfg(feature = "_tiles")]
use crate::srv::RESERVED_KEYWORDS;
use crate::{MartinError, MartinResult};

/// Warnings that can occur during tile source resolution
#[derive(thiserror::Error, Debug)]
pub enum TileSourceWarning {
    #[error("Source {source_id}: {error}")]
    SourceError { source_id: String, error: String },

    #[error("Path {path}: {error}")]
    PathError { path: String, error: String },
}

pub struct ServerState {
    #[cfg(feature = "_tiles")]
    pub tiles: TileSources,
    #[cfg(feature = "_tiles")]
    pub tile_cache: OptTileCache,

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
pub struct Config {
    /// Maximum size of the tile cache in megabytes (0 to disable)
    ///
    /// Can be overridden by [`tile_cache_size_mb`](Self::tile_cache_size_mb) or similar configuration options.
    pub cache_size_mb: Option<u64>,

    /// Maximum size of the tile cache in megabytes (0 to disable)
    ///
    /// Overrides [`cache_size_mb`](Self::cache_size_mb)
    pub tile_cache_size_mb: Option<u64>,

    #[serde(default)]
    pub on_invalid: Option<OnInvalid>,

    #[serde(flatten)]
    pub srv: super::srv::SrvConfig,

    #[cfg(feature = "postgres")]
    #[serde(default, skip_serializing_if = "OptOneMany::is_none")]
    pub postgres: OptOneMany<super::postgres::PostgresConfig>,

    #[cfg(feature = "pmtiles")]
    #[serde(default, skip_serializing_if = "FileConfigEnum::is_none")]
    pub pmtiles: FileConfigEnum<super::pmtiles::PmtConfig>,

    #[cfg(feature = "mbtiles")]
    #[serde(default, skip_serializing_if = "FileConfigEnum::is_none")]
    pub mbtiles: FileConfigEnum<super::mbtiles::MbtConfig>,

    #[cfg(feature = "unstable-cog")]
    #[serde(default, skip_serializing_if = "FileConfigEnum::is_none")]
    pub cog: FileConfigEnum<super::cog::CogConfig>,

    #[cfg(feature = "sprites")]
    #[serde(default, skip_serializing_if = "FileConfigEnum::is_none")]
    pub sprites: super::sprites::SpriteConfig,

    #[cfg(feature = "styles")]
    #[serde(default, skip_serializing_if = "FileConfigEnum::is_none")]
    pub styles: super::styles::StyleConfig,

    #[cfg(feature = "fonts")]
    #[serde(default, skip_serializing_if = "FileConfigEnum::is_none")]
    pub fonts: super::fonts::FontConfig,

    #[serde(flatten, skip_serializing)]
    pub unrecognized: UnrecognizedValues,
}

impl Config {
    /// Apply defaults to the config, and validate if there is a connection string
    pub fn finalize(&mut self) -> MartinResult<UnrecognizedKeys> {
        let mut res = self.srv.get_unrecognized_keys();
        copy_unrecognized_keys_from_config(&mut res, "", &self.unrecognized);

        if let Some(path) = &self.srv.base_path {
            self.srv.base_path = Some(parse_base_path(path)?);
        }
        #[cfg(feature = "_tiles")]
        if let Some(url) = &self.srv.base_url {
            self.srv.base_url = Some(parse_base_url(url)?);
        }
        #[cfg(feature = "postgres")]
        {
            let pg_prefix = if matches!(self.postgres, OptOneMany::One(_)) {
                "postgres."
            } else {
                "postgres[]."
            };
            for pg in self.postgres.iter_mut() {
                pg.finalize()?;
                res.extend(pg.get_unrecognized_keys_with_prefix(pg_prefix));
            }
        }

        #[cfg(feature = "pmtiles")]
        {
            // if a pmtiles source were to keep being configured like this,
            // we would not be able to migrate defaults/deprecate settings
            //
            // pmiles intialisation after this in resolve_tile_sources depends on this behaviour and will panic otherwise
            self.pmtiles = self.pmtiles.clone().into_config();
            self.pmtiles.finalize()?;
            res.extend(self.pmtiles.get_unrecognized_keys_with_prefix("pmtiles."));
        }

        #[cfg(feature = "mbtiles")]
        {
            self.mbtiles.finalize()?;
            res.extend(self.mbtiles.get_unrecognized_keys_with_prefix("mbtiles."));
        }

        #[cfg(feature = "unstable-cog")]
        {
            self.cog.finalize()?;
            res.extend(self.cog.get_unrecognized_keys_with_prefix("cog."));
        }

        #[cfg(feature = "sprites")]
        {
            self.sprites.finalize()?;
            res.extend(self.sprites.get_unrecognized_keys_with_prefix("sprites."));
        }

        #[cfg(feature = "styles")]
        {
            self.styles.finalize()?;
            res.extend(self.styles.get_unrecognized_keys_with_prefix("styles."));
        }

        // TODO: support for unrecognized fonts?
        // #[cfg(feature = "fonts")]
        // {
        //     self.fonts.finalize()?;
        //     res.extend(self.fonts.get_unrecognized_keys_with_prefix("fonts."));
        // }

        for key in &res {
            warn!(
                "Ignoring unrecognized configuration key '{key}'. Please check your configuration file for typos."
            );
        }

        let is_empty = true;

        #[cfg(feature = "postgres")]
        let is_empty = is_empty && self.postgres.is_empty();

        #[cfg(feature = "pmtiles")]
        let is_empty = is_empty && self.pmtiles.is_empty();

        #[cfg(feature = "mbtiles")]
        let is_empty = is_empty && self.mbtiles.is_empty();

        #[cfg(feature = "unstable-cog")]
        let is_empty = is_empty && self.cog.is_empty();

        #[cfg(feature = "sprites")]
        let is_empty = is_empty && self.sprites.is_empty();

        #[cfg(feature = "styles")]
        let is_empty = is_empty && self.styles.is_empty();

        #[cfg(feature = "fonts")]
        let is_empty = is_empty && self.fonts.is_empty();

        if is_empty {
            Err(ConfigFileError::NoSources.into())
        } else {
            Ok(res)
        }
    }

    pub async fn resolve(&mut self) -> MartinResult<ServerState> {
        init_aws_lc_tls();

        #[cfg(feature = "_tiles")]
        let resolver = IdResolver::new(RESERVED_KEYWORDS);

        #[cfg(any(feature = "_tiles", feature = "sprites", feature = "fonts"))]
        let cache_config = self.resolve_cache_config();

        #[cfg(feature = "pmtiles")]
        let pmtiles_cache = cache_config.create_pmtiles_cache();

        #[cfg(feature = "_tiles")]
        let (tiles, warnings) = self
            .resolve_tile_sources(
                &resolver,
                #[cfg(feature = "pmtiles")]
                pmtiles_cache,
            )
            .await?;

        #[cfg(feature = "_tiles")]
        self.on_invalid
            .unwrap_or_default()
            .handle_tile_warnings(&warnings)?;

        Ok(ServerState {
            #[cfg(feature = "_tiles")]
            tiles,
            #[cfg(feature = "_tiles")]
            tile_cache: cache_config.create_tile_cache(),

            #[cfg(feature = "sprites")]
            sprites: self.sprites.resolve()?,
            #[cfg(feature = "sprites")]
            sprite_cache: cache_config.create_sprite_cache(),

            #[cfg(feature = "fonts")]
            fonts: self.fonts.resolve()?,
            #[cfg(feature = "fonts")]
            font_cache: cache_config.create_font_cache(),

            #[cfg(feature = "styles")]
            styles: self.styles.resolve()?,
        })
    }

    // cache_config is still respected, but can be overridden by individual cache sizes
    //
    // `cache_config: 0` disables caching, unless overridden by individual cache sizes
    #[cfg(any(feature = "_tiles", feature = "sprites", feature = "fonts"))]
    fn resolve_cache_config(&self) -> CacheConfig {
        if let Some(cache_size_mb) = self.cache_size_mb {
            #[cfg(feature = "pmtiles")]
            let pmtiles_cache_size_mb = if let FileConfigEnum::Config(cfg) = &self.pmtiles {
                cfg.custom
                    .directory_cache_size_mb
                    .unwrap_or(cache_size_mb / 4) // Default: 25% for PMTiles directories
            } else {
                cache_size_mb / 4 // Default: 25% for PMTiles directories
            };

            #[cfg(feature = "sprites")]
            let sprite_cache_size_mb = if let FileConfigEnum::Config(cfg) = &self.sprites {
                cfg.custom.cache_size_mb.unwrap_or(cache_size_mb / 8) // Default: 12.5% for sprites
            } else {
                cache_size_mb / 8 // Default: 12.5% for sprites
            };

            #[cfg(feature = "fonts")]
            let font_cache_size_mb = if let FileConfigEnum::Config(cfg) = &self.fonts {
                cfg.custom.cache_size_mb.unwrap_or(cache_size_mb / 8) // Default: 12.5% for fonts
            } else {
                cache_size_mb / 8 // Default: 12.5% for fonts
            };

            CacheConfig {
                #[cfg(feature = "_tiles")]
                tile_cache_size_mb: self.tile_cache_size_mb.unwrap_or(cache_size_mb / 2), // Default: 50% for tiles
                #[cfg(feature = "pmtiles")]
                pmtiles_cache_size_mb,
                #[cfg(feature = "sprites")]
                sprite_cache_size_mb,
                #[cfg(feature = "fonts")]
                font_cache_size_mb,
            }
        } else {
            // TODO: the defaults could be smarter. If I don't have pmtiles sources, don't reserve cache for it
            CacheConfig {
                #[cfg(feature = "_tiles")]
                tile_cache_size_mb: 256,
                #[cfg(feature = "pmtiles")]
                pmtiles_cache_size_mb: 128,
                #[cfg(feature = "sprites")]
                sprite_cache_size_mb: 64,
                #[cfg(feature = "fonts")]
                font_cache_size_mb: 64,
            }
        }
    }

    #[cfg(feature = "_tiles")]
    async fn resolve_tile_sources(
        &mut self,
        #[allow(unused_variables)] idr: &IdResolver,
        #[cfg(feature = "pmtiles")] pmtiles_cache: PmtCache,
    ) -> MartinResult<(TileSources, Vec<TileSourceWarning>)> {
        let mut sources_and_warnings: Vec<BoxFuture<_>> = Vec::new();

        #[cfg(feature = "postgres")]
        for s in self.postgres.iter_mut() {
            sources_and_warnings.push(Box::pin(s.resolve(idr.clone())));
        }

        #[cfg(feature = "pmtiles")]
        if !self.pmtiles.is_empty() {
            let cfg = &mut self.pmtiles;
            match cfg {
                FileConfigEnum::None => {}
                FileConfigEnum::Paths(_) | FileConfigEnum::Path(_) => unreachable!(
                    "pmtiles was transformed to FileConfigEnum::Config in the previous step via `into_config`",
                ),
                FileConfigEnum::Config(file_config) => {
                    file_config.custom.pmtiles_directory_cache = pmtiles_cache;
                }
            }
            let val = crate::config::file::resolve_files(cfg, idr, &["pmtiles"]);
            sources_and_warnings.push(Box::pin(val));
        }

        #[cfg(feature = "mbtiles")]
        if !self.mbtiles.is_empty() {
            let cfg = &mut self.mbtiles;
            let val = crate::config::file::resolve_files(cfg, idr, &["mbtiles"]);
            sources_and_warnings.push(Box::pin(val));
        }

        #[cfg(feature = "unstable-cog")]
        if !self.cog.is_empty() {
            let cfg = &mut self.cog;
            let val = crate::config::file::resolve_files(cfg, idr, &["tif", "tiff"]);
            sources_and_warnings.push(Box::pin(val));
        }

        let all_results = try_join_all(sources_and_warnings).await?;
        let (all_tile_sources, all_tile_warnings): (Vec<_>, Vec<_>) =
            all_results.into_iter().unzip();

        Ok((
            TileSources::new(all_tile_sources),
            all_tile_warnings.into_iter().flatten().collect(),
        ))
    }

    pub fn save_to_file(&self, file_name: &Path) -> ConfigFileResult<()> {
        let yaml = serde_yaml::to_string(&self).expect("Unable to serialize config");
        if file_name.as_os_str() == OsStr::new("-") {
            info!("Current system configuration:");
            println!("\n\n{yaml}\n");
            Ok(())
        } else {
            info!(
                "Saving config to {}, use --config to load it",
                file_name.display()
            );
            File::create(file_name)
                .map_err(|e| ConfigFileError::ConfigWriteError(e, file_name.to_path_buf()))?
                .write_all(yaml.as_bytes())
                .map_err(|e| ConfigFileError::ConfigWriteError(e, file_name.to_path_buf()))?;
            Ok(())
        }
    }
}

/// Describes the action to take during startup when configuration is found to be invalid
/// but Martin could still startup in a degraded state (ie, some sources not served).
#[derive(PartialEq, Eq, Debug, Clone, Copy, Default, Serialize, Deserialize, ValueEnum)]
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
    pub fn handle_tile_warnings(self, warnings: &[TileSourceWarning]) -> MartinResult<()> {
        if warnings.is_empty() {
            return Ok(());
        }
        match warnings {
            [warning] => match self {
                OnInvalid::Warn => warn!("Tile source resolution warning: {warning}"),
                OnInvalid::Abort => error!("Tile source resolution warning: {warning}"),
            },
            warnings => match self {
                OnInvalid::Warn => warn!("Tile source resolutions:\n{}", fmt_warnings(warnings)),
                OnInvalid::Abort => error!("Tile source resolutions:\n{}", fmt_warnings(warnings)),
            },
        }

        match self {
            OnInvalid::Abort => Err(MartinError::TileResolutionWarningsIssued),
            OnInvalid::Warn => Ok(()),
        }
    }
}

/// Read config from a file
pub fn read_config<'a, M>(file_name: &Path, env: &'a M) -> ConfigFileResult<Config>
where
    M: VariableMap<'a>,
    M::Value: AsRef<str>,
{
    let mut file =
        File::open(file_name).map_err(|e| ConfigFileError::ConfigLoadError(e, file_name.into()))?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)
        .map_err(|e| ConfigFileError::ConfigLoadError(e, file_name.into()))?;
    parse_config(&contents, env, file_name)
}

pub fn parse_config<'a, M>(contents: &str, env: &'a M, file_name: &Path) -> ConfigFileResult<Config>
where
    M: VariableMap<'a>,
    M::Value: AsRef<str>,
{
    subst::yaml::from_str(contents, env)
        .map_err(|e| ConfigFileError::ConfigParseError(e, file_name.into()))
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

#[cfg(feature = "_tiles")]
pub fn parse_base_url(url: &str) -> MartinResult<String> {
    // Parse the URL to validate it
    match url.parse::<actix_web::http::Uri>() {
        Ok(uri) => {
            // Ensure the URL has a scheme and authority (host)
            if uri.scheme().is_none() {
                return Err(MartinError::BaseUrlError(format!(
                    "{url} - missing scheme (e.g., https://)"
                )));
            }
            if uri.authority().is_none() {
                return Err(MartinError::BaseUrlError(format!("{url} - missing host")));
            }
            // Normalize by removing trailing slashes for consistent URL formatting
            Ok(url.trim_end_matches('/').to_string())
        }
        Err(_) => Err(MartinError::BaseUrlError(url.to_string())),
    }
}

pub fn init_aws_lc_tls() {
    // https://github.com/rustls/rustls/issues/1877
    static INIT_TLS: LazyLock<()> = LazyLock::new(|| {
        rustls::crypto::aws_lc_rs::default_provider()
            .install_default()
            .expect("Unable to init rustls: {e:?}");
    });
    *INIT_TLS;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_base_path_accepts_valid_paths() {
        assert_eq!("", parse_base_path("/").unwrap());
        assert_eq!("", parse_base_path("//").unwrap());
        assert_eq!("/foo/bar", parse_base_path("/foo/bar").unwrap());
        assert_eq!("/foo/bar", parse_base_path("/foo/bar/").unwrap());
    }

    #[test]
    fn parse_base_path_rejects_invalid_paths() {
        assert!(parse_base_path("").is_err());
        assert!(parse_base_path("foo/bar").is_err());
    }

    #[cfg(feature = "_tiles")]
    #[test]
    fn parse_base_url_accepts_valid_urls() {
        assert_eq!(
            "https://example.com",
            parse_base_url("https://example.com").unwrap()
        );
        assert_eq!(
            "https://example.com",
            parse_base_url("https://example.com/").unwrap()
        );
        assert_eq!(
            "https://tiles.example.com",
            parse_base_url("https://tiles.example.com").unwrap()
        );
        assert_eq!(
            "https://example.com/tiles",
            parse_base_url("https://example.com/tiles").unwrap()
        );
        assert_eq!(
            "https://example.com/tiles",
            parse_base_url("https://example.com/tiles/").unwrap()
        );
        assert_eq!(
            "http://localhost:3000",
            parse_base_url("http://localhost:3000").unwrap()
        );
    }

    #[cfg(feature = "_tiles")]
    #[test]
    fn parse_base_url_rejects_invalid_urls() {
        // Missing scheme
        assert!(parse_base_url("example.com").is_err());
        assert!(parse_base_url("//example.com").is_err());
        // Missing host
        assert!(parse_base_url("https://").is_err());
        // Just a path
        assert!(parse_base_url("/tiles").is_err());
        // Empty
        assert!(parse_base_url("").is_err());
    }
}
