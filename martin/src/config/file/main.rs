use std::ffi::OsStr;
use std::fs::File;
use std::io::prelude::*;
#[cfg(any(feature = "_tiles", feature = "sprites", feature = "fonts"))]
use std::num::NonZeroU64;
use std::path::Path;
use std::sync::LazyLock;
#[cfg(any(test, feature = "_tiles", feature = "sprites", feature = "fonts"))]
use std::time::Duration;

use clap::ValueEnum;
#[cfg(feature = "_tiles")]
use futures::future::{BoxFuture, try_join_all};
#[cfg(feature = "pmtiles")]
use martin_core::tiles::pmtiles::PmtCache;
use serde::{Deserialize, Serialize};
use subst::VariableMap;
use tracing::{error, info, warn};

#[cfg(feature = "unstable-cog")]
use super::cog::CogConfig;
#[cfg(feature = "fonts")]
use super::fonts::FontConfig;
#[cfg(feature = "mbtiles")]
use super::mbtiles::MbtConfig;
#[cfg(feature = "pmtiles")]
use super::pmtiles::PmtConfig;
#[cfg(feature = "postgres")]
use super::postgres::PostgresConfig;
#[cfg(feature = "sprites")]
use super::sprites::SpriteConfig;
use super::srv::SrvConfig;
#[cfg(feature = "styles")]
use super::styles::StyleConfig;
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
use crate::config::file::cache::{CacheConfig, SubCacheSetting};
#[cfg(any(feature = "pmtiles", feature = "mbtiles", feature = "unstable-cog"))]
use crate::config::file::resolve_files;
use crate::config::file::{
    ConfigFileError, ConfigFileResult, ConfigurationLivecycleHooks as _, GlobalCacheConfig,
    UnrecognizedKeys, UnrecognizedValues, copy_unrecognized_keys_from_config,
};
#[cfg(feature = "_tiles")]
use crate::config::primitives::IdResolver;
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
    PathError { path: String, error: String },
}

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
pub struct Config {
    /// Cache configuration: size limits and default zoom-level bounds.
    #[serde(default, skip_serializing_if = "GlobalCacheConfig::is_empty")]
    pub cache: GlobalCacheConfig,

    #[serde(default)]
    pub on_invalid: Option<OnInvalid>,

    #[serde(flatten)]
    pub srv: SrvConfig,

    #[cfg(feature = "postgres")]
    #[serde(default, skip_serializing_if = "OptOneMany::is_none")]
    pub postgres: OptOneMany<PostgresConfig>,

    #[cfg(feature = "pmtiles")]
    #[serde(default, skip_serializing_if = "FileConfigEnum::is_none")]
    pub pmtiles: FileConfigEnum<PmtConfig>,

    #[cfg(feature = "mbtiles")]
    #[serde(default, skip_serializing_if = "FileConfigEnum::is_none")]
    pub mbtiles: FileConfigEnum<MbtConfig>,

    #[cfg(feature = "unstable-cog")]
    #[serde(default, skip_serializing_if = "FileConfigEnum::is_none")]
    pub cog: FileConfigEnum<CogConfig>,

    #[cfg(feature = "sprites")]
    #[serde(default, skip_serializing_if = "FileConfigEnum::is_none")]
    pub sprites: SpriteConfig,

    #[cfg(feature = "styles")]
    #[serde(default, skip_serializing_if = "FileConfigEnum::is_none")]
    pub styles: StyleConfig,

    #[cfg(feature = "fonts")]
    #[serde(default, skip_serializing_if = "FileConfigEnum::is_none")]
    pub fonts: FontConfig,

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
            // pmiles initialisation after this in resolve_tile_sources depends on this behaviour and will panic otherwise
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

    pub async fn resolve(
        &mut self,
        #[cfg(feature = "_tiles")] idr: &IdResolver,
    ) -> MartinResult<ServerState> {
        init_aws_lc_tls();

        #[cfg(any(feature = "_tiles", feature = "sprites", feature = "fonts"))]
        let cache_config = self.resolve_cache_config();

        #[cfg(feature = "pmtiles")]
        let pmtiles_cache = cache_config.create_pmtiles_cache();

        #[cfg(feature = "_tiles")]
        let (tile_sources, warnings) = self
            .resolve_tile_sources(
                idr,
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
            tile_manager: TileSourceManager::from_sources(
                cache_config.create_tile_cache(),
                self.on_invalid.unwrap_or_default(),
                tile_sources,
            ),

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

    // cache.size_mb is still respected, but can be overridden by individual cache sizes
    //
    // `cache.size_mb: 0` disables caching, unless overridden by individual cache sizes
    #[cfg(any(feature = "_tiles", feature = "sprites", feature = "fonts"))]
    fn resolve_cache_config(&self) -> CacheConfig {
        let global_expiry = self.cache.expiry;
        let global_idle = self.cache.idle_timeout;

        if let Some(cache_size_mb) = self.cache.size_mb {
            #[cfg(feature = "_tiles")]
            let tiles = Self::make_sub_cache(
                self.cache.tile_size_mb.unwrap_or(cache_size_mb / 2),
                self.cache.tile_expiry.or(global_expiry),
                self.cache.tile_idle_timeout.or(global_idle),
            );

            #[cfg(feature = "pmtiles")]
            let pmtiles = {
                let (size, expiry, idle) = if let FileConfigEnum::Config(cfg) = &self.pmtiles {
                    (
                        cfg.custom
                            .directory_cache
                            .size_mb
                            .unwrap_or(cache_size_mb / 4),
                        cfg.custom.directory_cache.expiry.or(global_expiry),
                        cfg.custom.directory_cache.idle_timeout.or(global_idle),
                    )
                } else {
                    (cache_size_mb / 4, global_expiry, global_idle)
                };
                Self::make_sub_cache(size, expiry, idle)
            };

            #[cfg(feature = "sprites")]
            let sprites = {
                let (size, expiry, idle) = if let FileConfigEnum::Config(cfg) = &self.sprites {
                    (
                        cfg.custom.cache.size_mb.unwrap_or(cache_size_mb / 8),
                        cfg.custom.cache.expiry.or(global_expiry),
                        cfg.custom.cache.idle_timeout.or(global_idle),
                    )
                } else {
                    (cache_size_mb / 8, global_expiry, global_idle)
                };
                Self::make_sub_cache(size, expiry, idle)
            };

            #[cfg(feature = "fonts")]
            let fonts = {
                let (size, expiry, idle) = if let FileConfigEnum::Config(cfg) = &self.fonts {
                    (
                        cfg.custom.cache.size_mb.unwrap_or(cache_size_mb / 8),
                        cfg.custom.cache.expiry.or(global_expiry),
                        cfg.custom.cache.idle_timeout.or(global_idle),
                    )
                } else {
                    (cache_size_mb / 8, global_expiry, global_idle)
                };
                Self::make_sub_cache(size, expiry, idle)
            };

            CacheConfig {
                #[cfg(feature = "_tiles")]
                tiles,
                #[cfg(feature = "pmtiles")]
                pmtiles,
                #[cfg(feature = "sprites")]
                sprites,
                #[cfg(feature = "fonts")]
                fonts,
            }
        } else {
            // TODO: the defaults could be smarter. If I don't have pmtiles sources, don't reserve cache for it
            CacheConfig {
                #[cfg(feature = "_tiles")]
                tiles: Self::make_sub_cache(
                    256,
                    self.cache.tile_expiry.or(global_expiry),
                    self.cache.tile_idle_timeout.or(global_idle),
                ),
                #[cfg(feature = "pmtiles")]
                pmtiles: Self::make_sub_cache(128, global_expiry, global_idle),
                #[cfg(feature = "sprites")]
                sprites: Self::make_sub_cache(64, global_expiry, global_idle),
                #[cfg(feature = "fonts")]
                fonts: Self::make_sub_cache(64, global_expiry, global_idle),
            }
        }
    }

    /// Helper to create a `SubCacheSetting` from size in MB. Returns `None` if size is 0.
    #[cfg(any(feature = "_tiles", feature = "sprites", feature = "fonts"))]
    fn make_sub_cache(
        size_mb: u64,
        expiry: Option<Duration>,
        idle_timeout: Option<Duration>,
    ) -> Option<SubCacheSetting> {
        NonZeroU64::new(size_mb).map(|size_mb| SubCacheSetting {
            size_mb,
            expiry,
            idle_timeout,
        })
    }

    #[cfg(feature = "_tiles")]
    async fn resolve_tile_sources(
        &mut self,
        idr: &IdResolver,
        #[cfg(feature = "pmtiles")] pmtiles_cache: PmtCache,
    ) -> MartinResult<(
        Vec<Vec<martin_core::tiles::BoxedSource>>,
        Vec<TileSourceWarning>,
    )> {
        let mut sources_and_warnings: Vec<BoxFuture<_>> = Vec::new();

        #[cfg(feature = "postgres")]
        for s in self.postgres.iter_mut() {
            sources_and_warnings.push(Box::pin(s.resolve(idr.clone(), self.cache.policy())));
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
            let val = resolve_files(cfg, idr, &["pmtiles"], self.cache.policy());
            sources_and_warnings.push(Box::pin(val));
        }

        #[cfg(feature = "mbtiles")]
        if !self.mbtiles.is_empty() {
            let cfg = &mut self.mbtiles;
            let val = resolve_files(cfg, idr, &["mbtiles"], self.cache.policy());
            sources_and_warnings.push(Box::pin(val));
        }

        #[cfg(feature = "unstable-cog")]
        if !self.cog.is_empty() {
            let cfg = &mut self.cog;
            let val = resolve_files(cfg, idr, &["tif", "tiff"], self.cache.policy());
            sources_and_warnings.push(Box::pin(val));
        }

        let all_results = try_join_all(sources_and_warnings).await?;
        let (all_tile_sources, all_tile_warnings): (Vec<_>, Vec<_>) =
            all_results.into_iter().unzip();

        Ok((
            all_tile_sources,
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
    // Phase 1: substitute environment variables at the text level so saphyr's spans line up
    // with the post-substitution text the parser actually sees.
    let substituted = subst::substitute(contents, env)
        .map_err(|e| ConfigFileError::substitution(e, contents.to_string(), file_name.into()))?;

    // Phase 2: rewrite deprecated cache keys via a `serde_yaml::Value` round-trip — but only
    // if at least one deprecated token appears in the text. The common case (no deprecated
    // keys) skips a full YAML parse + serialize.
    let migrated = if needs_deprecated_migration(&substituted) {
        match serde_yaml::from_str::<serde_yaml::Value>(&substituted) {
            Ok(mut value) => {
                migrate_deprecated_config(&mut value);
                serde_yaml::to_string(&value).unwrap_or(substituted)
            }
            // If serde_yaml itself can't parse, hand the original to saphyr — its diagnostics
            // are richer, so let it produce the user-facing error.
            Err(_) => substituted,
        }
    } else {
        substituted
    };

    // Phase 3: parse to the typed `Config` via saphyr. We disable saphyr's built-in snippet
    // wrapper so its hardcoded `<input>` source name doesn't override the file path we show;
    // `ConfigFileError::to_miette_report` re-attaches a snippet against our own NamedSource.
    let options = serde_saphyr::options! {
        with_snippet: false,
    };
    serde_saphyr::from_str_with_options::<Config>(&migrated, options)
        .map_err(|e| ConfigFileError::yaml_parse(e, migrated, file_name.into()))
}

/// Cheap pre-check: does the substituted YAML mention any deprecated cache key?
///
/// False positives are harmless (the fast path is identical to the slow path's no-op
/// migration), so a substring search is sufficient.
fn needs_deprecated_migration(yaml: &str) -> bool {
    yaml.contains("cache_size_mb")
        || yaml.contains("tile_cache_size_mb")
        || yaml.contains("directory_cache_size_mb")
}

/// Migrates deprecated cache configuration keys in raw YAML before deserialization.
///
/// This runs on the `serde_yaml::Value` directly, so the `Config` struct
/// never needs to know about deprecated field names.
fn migrate_deprecated_config(value: &mut serde_yaml::Value) {
    let Some(root) = value.as_mapping_mut() else {
        return;
    };

    // Global: cache_size_mb -> cache.size_mb
    migrate_yaml_key(root, "cache_size_mb", &["cache", "size_mb"]);

    // Global: tile_cache_size_mb -> cache.tile_size_mb
    migrate_yaml_key(root, "tile_cache_size_mb", &["cache", "tile_size_mb"]);

    // Source-type level: {section}.cache_size_mb -> {section}.cache.size_mb
    for section in ["sprites", "fonts"] {
        if let Some(mapping) = root
            .get_mut(serde_yaml::Value::String(section.into()))
            .and_then(|v| v.as_mapping_mut())
        {
            migrate_yaml_key(mapping, "cache_size_mb", &["cache", "size_mb"]);
        }
    }

    // PMTiles: directory_cache_size_mb -> directory_cache.size_mb
    if let Some(mapping) = root
        .get_mut(serde_yaml::Value::String("pmtiles".into()))
        .and_then(|v| v.as_mapping_mut())
    {
        migrate_yaml_key(
            mapping,
            "directory_cache_size_mb",
            &["directory_cache", "size_mb"],
        );
    }
}

/// Moves a deprecated key in a YAML mapping to a new nested location.
///
/// `new_path` is a slice of keys describing the nested destination,
/// e.g. `&["cache", "size_mb"]` means `cache.size_mb`.
///
/// If the new key already exists, the old value is dropped with a warning.
/// If only the old key exists, it is moved to the new location.
fn migrate_yaml_key(mapping: &mut serde_yaml::Mapping, old_key: &str, new_path: &[&str]) {
    debug_assert!(!new_path.is_empty(), "new_path must not be empty");

    let old_yaml_key = serde_yaml::Value::String(old_key.into());
    let Some(old_value) = mapping.remove(&old_yaml_key) else {
        return;
    };

    let new_key_display = new_path.join(".");

    // Walk down to the parent of the leaf key, creating intermediate mappings as needed
    let [parents @ .., leaf] = new_path else {
        return;
    };
    let mut current = &mut *mapping;
    for &segment in parents {
        if !current.contains_key(segment) {
            current.insert(
                serde_yaml::Value::String(segment.into()),
                serde_yaml::Value::Mapping(serde_yaml::Mapping::default()),
            );
        }
        let Some(nested) = current.get_mut(segment).and_then(|v| v.as_mapping_mut()) else {
            warn!(
                "deprecated config: `{old_key}` is ignored because `{segment}` is already set. \
                 Please remove `{old_key}` from your configuration"
            );
            return;
        };
        current = nested;
    }

    if current.contains_key(leaf) {
        warn!(
            "deprecated config: `{old_key}` is ignored in favor of `{new_key_display}`. \
             Please remove `{old_key}` from your configuration"
        );
    } else {
        current.insert(serde_yaml::Value::String((*leaf).into()), old_value);
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
    use std::collections::HashMap;

    use martin_core::CacheZoomRange;

    use super::*;
    use crate::config::file::CachePolicy;
    use crate::config::test_helpers::render_failure;

    fn parse_yaml(yaml: &str) -> Config {
        parse_config(
            yaml,
            &HashMap::<String, String>::new(),
            Path::new("test.yaml"),
        )
        .unwrap()
    }

    // ----- `parse_config` pipeline diagnostics: failures that don't belong to a single
    // ----- field's deserializer (raw YAML syntax, ${VAR} substitution, derive-`Deserialize`
    // ----- enums) live here next to the function under test.

    #[test]
    fn syntax_error_unbalanced_quote() {
        insta::assert_snapshot!(
            render_failure(indoc::indoc! {r#"
                srv:
                  listen_addresses: "0.0.0.0:3000
                  worker_processes: 4
            "#}),
            @r#"
         × invalid indentation in multiline quoted scalar
          ╭─[config.yaml:3:3]
        2 │   listen_addresses: "0.0.0.0:3000
        3 │   worker_processes: 4
          ·   ┬
          ·   ╰── invalid indentation in multiline quoted scalar
          ╰────
        "#
        );
    }

    #[test]
    fn unknown_enum_variant_in_on_invalid() {
        insta::assert_snapshot!(render_failure("on_invalid: maybe\n"), @"
         × unknown variant `maybe`, expected one of continue, ignore, warn, warning,
         │ warnings, abort
          ╭─[config.yaml:1:13]
        1 │ on_invalid: maybe
          ·             ──┬──
          ·               ╰── unknown variant `maybe`, expected one of continue, ignore, warn, warning, warnings, abort
          ╰────
        ");
    }

    #[test]
    fn substitution_undefined_variable() {
        insta::assert_snapshot!(render_failure("cache_size_mb: ${UNDEFINED_VAR}\n"), @"
        martin::config::substitution (https://maplibre.org/martin/config-file/)

          × Unable to substitute environment variables in config file config.yaml: No
          │ such variable: $UNDEFINED_VAR
           ╭─[config.yaml:1:18]
         1 │ cache_size_mb: ${UNDEFINED_VAR}
           ·                  ──────┬──────
           ·                        ╰── No such variable: $UNDEFINED_VAR
           ╰────
          help: Make sure every ${VAR} reference resolves to an environment variable,
                or supply a default with `${VAR:-fallback}`.
        ");
    }

    #[test]
    fn cors_unsupported_scalar_renders_as_json() {
        // Mirrors what the binary emits when `RUST_LOG_FORMAT=json` is set: a structured
        // JSON document instead of the graphical snippet, suitable for editor tooling and
        // log aggregators.
        let json = crate::config::test_helpers::render_failure_json("cors: 42\n");
        let parsed: serde_json::Value =
            serde_json::from_str(&json).unwrap_or_else(|e| panic!("not JSON: {e}\n{json}"));

        let message = parsed.get("message").and_then(|m| m.as_str()).unwrap_or("");
        assert!(
            message.contains("invalid type: integer `42`"),
            "unexpected message in JSON output: {message}"
        );
        assert_eq!(
            parsed.get("severity").and_then(|s| s.as_str()),
            Some("error")
        );
        assert_eq!(
            parsed.get("filename").and_then(|f| f.as_str()),
            Some("config.yaml")
        );
        let labels = parsed.get("labels").and_then(|l| l.as_array()).unwrap();
        assert_eq!(labels.len(), 1, "expected one label, got {labels:?}");
        let span = labels[0].get("span").unwrap();
        assert!(span.get("offset").is_some(), "label missing offset");
        assert!(span.get("length").is_some(), "label missing length");
    }

    #[test]
    fn substitution_renders_as_json_with_code_help_url() {
        // The substitution path uses our own `SubstitutionDiagnostic`, which overrides
        // `code()`, `help()`, and `url()`. The JSON renderer surfaces all three.
        let json =
            crate::config::test_helpers::render_failure_json("cache_size_mb: ${UNDEFINED_VAR}\n");
        let parsed: serde_json::Value =
            serde_json::from_str(&json).unwrap_or_else(|e| panic!("not JSON: {e}\n{json}"));

        assert_eq!(
            parsed.get("code").and_then(|c| c.as_str()),
            Some("martin::config::substitution")
        );
        let help = parsed.get("help").and_then(|h| h.as_str()).unwrap_or("");
        assert!(
            help.contains("${VAR}"),
            "expected help text mentioning ${{VAR}}, got: {help}"
        );
        assert_eq!(
            parsed.get("url").and_then(|u| u.as_str()),
            Some("https://maplibre.org/martin/config-file/")
        );
    }

    #[test]
    fn non_spanned_error_renders_as_json_envelope() {
        // For errors that don't carry source location info, JSON mode still emits a JSON
        // document so downstream tools can keep parsing rather than choking on a free-form
        // log line.
        let envelope = MartinError::BasePathError("not-a-path".to_string())
            .render_diagnostic_with(crate::logging::LogFormat::Json);
        let parsed: serde_json::Value =
            serde_json::from_str(&envelope).unwrap_or_else(|e| panic!("not JSON: {e}\n{envelope}"));
        let msg = parsed.get("message").and_then(|m| m.as_str()).unwrap_or("");
        assert!(
            msg.contains("not-a-path"),
            "expected envelope to include the error message; got: {envelope}"
        );
    }

    #[test]
    fn substitution_unclosed_brace() {
        insta::assert_snapshot!(render_failure("cache_size_mb: ${BROKEN\n"), @r"
        martin::config::substitution (https://maplibre.org/martin/config-file/)

          × Unable to substitute environment variables in config file config.yaml:
          │ Unexpected character: '\n', expected a closing brace ('}') or colon (':')
           ╭─[config.yaml:1:24]
         1 │ cache_size_mb: ${BROKEN
           ·                        ┬
           ·                        ╰── Unexpected character: '\n', expected a closing brace ('}') or colon (':')
           ╰────
          help: Make sure every ${VAR} reference resolves to an environment variable,
                or supply a default with `${VAR:-fallback}`.
        ");
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
        assert!(parse_base_path("").is_err());
        assert!(parse_base_path("foo/bar").is_err());
    }

    #[test]
    fn cache_migrates_old_to_new_cache_config_key() {
        let config = parse_yaml("cache_size_mb: 512");
        assert_eq!(config.cache.size_mb, Some(512));
    }

    #[test]
    fn migrate_tile_cache_size_mb_to_cache_tile_size_mb() {
        let config = parse_yaml("tile_cache_size_mb: 256");
        assert_eq!(config.cache.tile_size_mb, Some(256));
    }

    #[test]
    fn migrate_both_old_cache_keys() {
        let config = parse_yaml("cache_size_mb: 512\ntile_cache_size_mb: 256");
        assert_eq!(config.cache.size_mb, Some(512));
        assert_eq!(config.cache.tile_size_mb, Some(256));
    }

    #[test]
    fn new_cache_key_overrides_old() {
        let config = parse_yaml("cache_size_mb: 100\ncache:\n  size_mb: 200");
        assert_eq!(config.cache.size_mb, Some(200));
    }

    #[test]
    fn new_cache_format_works_directly() {
        let config =
            parse_yaml("cache:\n  size_mb: 512\n  tile_size_mb: 256\n  minzoom: 2\n  maxzoom: 10");
        assert_eq!(config.cache.size_mb, Some(512));
        assert_eq!(config.cache.tile_size_mb, Some(256));
    }

    #[cfg(feature = "sprites")]
    #[test]
    fn migrate_sprites_cache_size_mb() {
        let config = parse_yaml("sprites:\n  cache_size_mb: 64\n  paths: /tmp");
        let FileConfigEnum::Config(cfg) = &config.sprites else {
            panic!("expected sprites config");
        };
        assert_eq!(cfg.custom.cache.size_mb, Some(64));
    }

    #[cfg(feature = "fonts")]
    #[test]
    fn migrate_fonts_cache_size_mb() {
        let config = parse_yaml("fonts:\n  cache_size_mb: 32\n  paths: /tmp");
        let FileConfigEnum::Config(cfg) = &config.fonts else {
            panic!("expected fonts config");
        };
        assert_eq!(cfg.custom.cache.size_mb, Some(32));
    }

    #[test]
    fn migrate_skips_non_mapping_intermediate() {
        // `cache: true` is not a mapping, so migration of cache_size_mb should
        // gracefully skip rather than panic, and the parse should still succeed
        // (cache will be deserialized from whatever value it has).
        let result = parse_config(
            "cache: true\ncache_size_mb: 100",
            &HashMap::<String, String>::new(),
            Path::new("test.yaml"),
        );
        // The parse may fail (cache: true is not a valid GlobalCacheConfig),
        // but it must not panic.
        let _ = result;
    }

    #[test]
    fn cache_disable_global() {
        let config = parse_yaml("cache: disable");
        assert_eq!(config.cache, GlobalCacheConfig::disabled());
        assert_eq!(config.cache.size_mb, Some(0));
        assert_eq!(config.cache.tile_size_mb, Some(0));
    }

    #[test]
    fn cache_disable_per_source() {
        let policy: CachePolicy = serde_yaml::from_str("disable").unwrap();
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

    #[test]
    fn cache_disable_global_propagates_to_unconfigured_source() {
        // Parse a global `cache: disable` and verify it propagates to a source with no cache config
        let config = parse_yaml("cache: disable");
        let global_policy = config.cache.policy();
        let unconfigured_source = CachePolicy::default();
        let merged = unconfigured_source.or(global_policy);
        for zoom in 0..=u8::MAX {
            assert!(!merged.zoom().contains(zoom));
        }
    }

    #[cfg(feature = "sprites")]
    #[test]
    fn cache_disable_sprites() {
        let config = parse_yaml("sprites:\n  cache: disable\n  paths: /tmp");
        let FileConfigEnum::Config(cfg) = &config.sprites else {
            panic!("expected sprites config");
        };
        assert_eq!(cfg.custom.cache.size_mb, Some(0));
    }

    #[test]
    fn cache_expiry_global_config() {
        let config = parse_yaml("cache:\n  size_mb: 512\n  expiry: 1h\n  idle_timeout: 15m");
        assert_eq!(config.cache.size_mb, Some(512));
        assert_eq!(config.cache.expiry, Some(Duration::from_hours(1)));
        assert_eq!(config.cache.idle_timeout, Some(Duration::from_mins(15)));
    }

    #[test]
    fn cache_expiry_tile_specific() {
        let config = parse_yaml(
            "cache:\n  expiry: 1h\n  idle_timeout: 15m\n  tile_expiry: 30m\n  tile_idle_timeout: 5m",
        );
        assert_eq!(config.cache.expiry, Some(Duration::from_hours(1)));
        assert_eq!(config.cache.tile_expiry, Some(Duration::from_mins(30)));
        assert_eq!(config.cache.tile_idle_timeout, Some(Duration::from_mins(5)));
    }

    #[test]
    fn cache_expiry_none_when_unset() {
        let config = parse_yaml("cache:\n  size_mb: 512");
        assert_eq!(config.cache.expiry, None);
        assert_eq!(config.cache.idle_timeout, None);
        assert_eq!(config.cache.tile_expiry, None);
        assert_eq!(config.cache.tile_idle_timeout, None);
    }

    #[cfg(feature = "sprites")]
    #[test]
    fn cache_expiry_sprites() {
        let config = parse_yaml(
            "sprites:\n  cache:\n    size_mb: 64\n    expiry: 2h\n    idle_timeout: 30m\n  paths: /tmp",
        );
        let FileConfigEnum::Config(cfg) = &config.sprites else {
            panic!("expected sprites config");
        };
        assert_eq!(cfg.custom.cache.size_mb, Some(64));
        assert_eq!(cfg.custom.cache.expiry, Some(Duration::from_hours(2)));
        assert_eq!(cfg.custom.cache.idle_timeout, Some(Duration::from_mins(30)));
    }
}
