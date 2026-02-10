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
use crate::config::file::cache::{CacheConfig, ResolvedCacheConfig};
use crate::config::file::{
    ConfigFileError, ConfigFileResult, ConfigurationLivecycleHooks as _, UnrecognizedKeys,
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
    /// DEPRECATED. USE `cache.size` instead
    pub cache_size_mb: Option<u64>,
    /// DEPRECATED. USE `cache.tiles.size` instead
    pub tile_cache_size_mb: Option<u64>,

    /// How the caching should be handled by martin
    #[serde(default, skip_serializing_if = "CacheConfig::is_none")]
    #[cfg(any(feature = "_tiles", feature = "sprites", feature = "fonts"))]
    pub cache: CacheConfig,

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

        if let Some(path) = &self.srv.route_prefix {
            let normalized = parse_base_path(path)?;
            // For route_prefix, an empty normalized path (from "/") means no prefix
            self.srv.route_prefix = if normalized.is_empty() {
                None
            } else {
                Some(normalized)
            };
        }
        if let Some(path) = &self.srv.base_path {
            self.srv.base_path = Some(parse_base_path(path)?);
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
        let pmtile_directorys_cache = cache_config.create_pmtile_directorys_cache();

        #[cfg(feature = "_tiles")]
        let (tiles, warnings) = self
            .resolve_tile_sources(
                &resolver,
                #[cfg(feature = "pmtiles")]
                pmtile_directorys_cache,
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

    /// Resolves which cache gets how much memory
    ///
    /// Before the decision to centralise cache configuration in self.cache, we had caching in different places
    /// For backwards compatibility, we need to ensure that the cache configuration is resolved correctly
    /// This also handles the default states and overrides
    #[cfg(any(feature = "_tiles", feature = "sprites", feature = "fonts"))]
    fn resolve_cache_config(&mut self) -> ResolvedCacheConfig {
        // we .take() below in case the configuration is serialized

        if let Some(old_size_mb) = self.cache_size_mb.take() {
            let cache = self.cache.object_mut();
            cache.size = Self::legacy_cache_config_item_helper(
                cache.size,
                "cache.size",
                old_size_mb,
                "cache_size_mb",
            );
        }

        #[cfg(feature = "_tiles")]
        if let Some(old_size_mb) = self.tile_cache_size_mb.take() {
            let cache = self.cache.object_mut();
            cache.tiles.size = Self::legacy_cache_config_item_helper(
                cache.tiles.size,
                "cache.tiles.size",
                old_size_mb,
                "tile_cache_size_mb",
            );
        }
        #[cfg(feature = "pmtiles")]
        if let Some(old_size_mb) = self
            .pmtiles
            .as_config_opt_mut()
            .and_then(|c| c.directory_cache_size_mb.take())
        {
            let cache = self.cache.object_mut();
            cache.pmtile_directorys.size = Self::legacy_cache_config_item_helper(
                cache.pmtile_directorys.size,
                "cache.pmtile_directorys.size",
                old_size_mb,
                "pmtiles_directory_cache",
            );
        }
        #[cfg(feature = "sprites")]
        if let Some(old_size_mb) = self
            .sprites
            .as_config_opt_mut()
            .and_then(|c| c.cache_size_mb.take())
        {
            let cache = self.cache.object_mut();
            cache.sprites.size = Self::legacy_cache_config_item_helper(
                cache.sprites.size,
                "cache.sprites.size",
                old_size_mb,
                "sprites.cache_size_mb",
            );
        }
        #[cfg(feature = "fonts")]
        if let Some(old_size_mb) = self
            .fonts
            .as_config_opt_mut()
            .and_then(|c| c.cache_size_mb.take())
        {
            let cache = self.cache.object_mut();
            cache.fonts.size = Self::legacy_cache_config_item_helper(
                cache.fonts.size,
                "cache.fonts.size",
                old_size_mb,
                "fonts.cache_size_mb",
            );
        }

        ResolvedCacheConfig::from(self.cache.clone())
    }
    #[cfg(any(feature = "_tiles", feature = "sprites", feature = "fonts"))]
    fn legacy_cache_config_item_helper(
        intended_size_bytes: Option<u64>,
        intended_label: &'static str,
        old_size_mb: u64,
        old_label: &'static str,
    ) -> Option<u64> {
        if let Some(size_bytes) = intended_size_bytes {
            warn!(
                "Both {intended_label} and {old_label} are set. Using {intended_label} and ignoring {old_label}."
            );
            Some(size_bytes)
        } else {
            warn!(
                "{old_label} configuration is deprecated. Please consider using `{intended_label}: {old_size_mb}MB` instead."
            );
            Some(old_size_mb * 1000 * 1000)
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

    #[cfg(all(
        feature = "_tiles",
        feature = "pmtiles",
        feature = "sprites",
        feature = "fonts"
    ))]
    mod cache_config {
        use indoc::indoc;

        use super::*;

        #[test]
        fn default() {
            let mut cfg = serde_yaml::from_str::<Config>(indoc! {"
            "})
            .unwrap();
            let cache = cfg.resolve_cache_config();
            insta::assert_debug_snapshot!(cache, @"
            ResolvedCacheConfig {
                tiles: Some(
                    ResolvedSubCacheSetting {
                        size_mb: 256,
                    },
                ),
                pmtile_directorys: Some(
                    ResolvedSubCacheSetting {
                        size_mb: 128,
                    },
                ),
                sprites: Some(
                    ResolvedSubCacheSetting {
                        size_mb: 64,
                    },
                ),
                fonts: Some(
                    ResolvedSubCacheSetting {
                        size_mb: 64,
                    },
                ),
            }
            ");
            insta::assert_yaml_snapshot!(cfg, @"{}");
        }

        #[test]
        fn global_disabling() {
            let mut cfg = serde_yaml::from_str::<Config>(indoc! {"
              cache: false
            "})
            .unwrap();
            let cache = cfg.resolve_cache_config();
            insta::assert_debug_snapshot!(cache, @"
              ResolvedCacheConfig {
                  tiles: None,
                  pmtile_directorys: None,
                  sprites: None,
                  fonts: None,
              }
              ");
            insta::assert_yaml_snapshot!(cfg, @"cache: false");
        }

        #[test]
        fn global_enabling() {
            let mut cfg = serde_yaml::from_str::<Config>(indoc! {"
              cache: true
            "})
            .unwrap();
            let cache = cfg.resolve_cache_config();
            insta::assert_debug_snapshot!(cache, @"
            ResolvedCacheConfig {
                tiles: Some(
                    ResolvedSubCacheSetting {
                        size_mb: 256,
                    },
                ),
                pmtile_directorys: Some(
                    ResolvedSubCacheSetting {
                        size_mb: 128,
                    },
                ),
                sprites: Some(
                    ResolvedSubCacheSetting {
                        size_mb: 64,
                    },
                ),
                fonts: Some(
                    ResolvedSubCacheSetting {
                        size_mb: 64,
                    },
                ),
            }
            ");
            insta::assert_yaml_snapshot!(cfg, @"cache: true");
        }

        #[test]
        fn old_cache_config() {
            let mut cfg = serde_yaml::from_str::<Config>(indoc! {"
              tile_cache_size_mb: 5
              pmtiles:
                directory_cache_size_mb: 6
              sprites:
                cache_size_mb: 7
              fonts:
                cache_size_mb: 8
                    "})
            .unwrap();
            let cache = cfg.resolve_cache_config();
            insta::assert_debug_snapshot!(cache, @"
            ResolvedCacheConfig {
                tiles: Some(
                    ResolvedSubCacheSetting {
                        size_mb: 5,
                    },
                ),
                pmtile_directorys: Some(
                    ResolvedSubCacheSetting {
                        size_mb: 6,
                    },
                ),
                sprites: Some(
                    ResolvedSubCacheSetting {
                        size_mb: 7,
                    },
                ),
                fonts: Some(
                    ResolvedSubCacheSetting {
                        size_mb: 8,
                    },
                ),
            }
            ");

            insta::assert_yaml_snapshot!(cfg, @"
            cache:
              tiles:
                size: 5.0MB
              pmtile_directorys:
                size: 6.0MB
              sprites:
                size: 7.0MB
              fonts:
                size: 8.0MB
            pmtiles: {}
            sprites: {}
            fonts: {}
            ");
        }

        #[test]
        fn new_cache_config() {
            let mut cfg = serde_yaml::from_str::<Config>(indoc! {"
              cache:
                tiles:
                  size: 1MB
                pmtile_directorys:
                  size: 2MB
                sprites:
                  size: 3MB
                fonts:
                  size: 4MB
                    "})
            .unwrap();
            let cache = cfg.resolve_cache_config();
            insta::assert_debug_snapshot!(cache, @"
            ResolvedCacheConfig {
                tiles: Some(
                    ResolvedSubCacheSetting {
                        size_mb: 1,
                    },
                ),
                pmtile_directorys: Some(
                    ResolvedSubCacheSetting {
                        size_mb: 2,
                    },
                ),
                sprites: Some(
                    ResolvedSubCacheSetting {
                        size_mb: 3,
                    },
                ),
                fonts: Some(
                    ResolvedSubCacheSetting {
                        size_mb: 4,
                    },
                ),
            }
            ");
            insta::assert_yaml_snapshot!(cfg, @"
            cache:
              tiles:
                size: 1.0MB
              pmtile_directorys:
                size: 2.0MB
              sprites:
                size: 3.0MB
              fonts:
                size: 4.0MB
            ");
        }

        #[test]
        fn new_and_old_config_defaults_to_new() {
            let mut cfg = serde_yaml::from_str::<Config>(indoc! {"
              # new
              cache:
                tiles:
                  size: 1MB
                pmtile_directorys:
                  size: 2MB
                sprites:
                  size: 3MB
                fonts:
                  size: 4MB

              # legacy
              tile_cache_size_mb: 5
              pmtiles:
                directory_cache_size_mb: 6
              sprites:
                cache_size_mb: 7
              fonts:
                cache_size_mb: 8
            "})
            .unwrap();
            let cache = cfg.resolve_cache_config();
            insta::assert_debug_snapshot!(cache, @"
            ResolvedCacheConfig {
                tiles: Some(
                    ResolvedSubCacheSetting {
                        size_mb: 1,
                    },
                ),
                pmtile_directorys: Some(
                    ResolvedSubCacheSetting {
                        size_mb: 2,
                    },
                ),
                sprites: Some(
                    ResolvedSubCacheSetting {
                        size_mb: 3,
                    },
                ),
                fonts: Some(
                    ResolvedSubCacheSetting {
                        size_mb: 4,
                    },
                ),
            }
            ");
            insta::assert_yaml_snapshot!(cfg, @"
            cache:
              tiles:
                size: 1.0MB
              pmtile_directorys:
                size: 2.0MB
              sprites:
                size: 3.0MB
              fonts:
                size: 4.0MB
            pmtiles: {}
            sprites: {}
            fonts: {}
            ");
        }

        #[test]
        fn cache_can_be_disabled_via_individual_size() {
            let mut cfg = serde_yaml::from_str::<Config>(indoc! {"
              cache:
                size: 1GB
                tiles:
                  size: 0B
                pmtile_directorys:
                  size: 0B
                sprites:
                  size: 0B
                fonts:
                  size: 0B
            "})
            .unwrap();
            let cache = cfg.resolve_cache_config();
            insta::assert_debug_snapshot!(cache, @"
              ResolvedCacheConfig {
                  tiles: None,
                  pmtile_directorys: None,
                  sprites: None,
                  fonts: None,
              }
              ");
            insta::assert_yaml_snapshot!(cfg, @"
              cache:
                size: 1.0GB
                tiles:
                  size: 0B
                pmtile_directorys:
                  size: 0B
                sprites:
                  size: 0B
                fonts:
                  size: 0B
              ");
        }

        #[test]
        fn individual_sizes_override_global_size() {
            let mut cfg = serde_yaml::from_str::<Config>(indoc! {"
              cache:
                size: 0B
                tiles:
                  size: 1MB
                pmtile_directorys:
                  size: 2MB
                sprites:
                  size: 3MB
                fonts:
                  size: 4MB
            "})
            .unwrap();
            let cache = cfg.resolve_cache_config();
            insta::assert_debug_snapshot!(cache, @"
            ResolvedCacheConfig {
                tiles: Some(
                    ResolvedSubCacheSetting {
                        size_mb: 1,
                    },
                ),
                pmtile_directorys: Some(
                    ResolvedSubCacheSetting {
                        size_mb: 2,
                    },
                ),
                sprites: Some(
                    ResolvedSubCacheSetting {
                        size_mb: 3,
                    },
                ),
                fonts: Some(
                    ResolvedSubCacheSetting {
                        size_mb: 4,
                    },
                ),
            }
            ");
            insta::assert_yaml_snapshot!(cfg, @"
            cache:
              size: 0B
              tiles:
                size: 1.0MB
              pmtile_directorys:
                size: 2.0MB
              sprites:
                size: 3.0MB
              fonts:
                size: 4.0MB
            ");
        }
    }
}
