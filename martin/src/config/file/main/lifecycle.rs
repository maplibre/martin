#[cfg(feature = "_tiles")]
use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs::File;
use std::io::prelude::*;
#[cfg(any(feature = "_tiles", feature = "sprites", feature = "fonts"))]
use std::num::NonZeroU64;
use std::path::Path;
#[cfg(any(feature = "_tiles", feature = "sprites", feature = "fonts"))]
use std::time::Duration;

#[cfg(feature = "_tiles")]
use futures::future::{BoxFuture, try_join_all};
#[cfg(feature = "_tiles")]
use martin_core::tiles::BoxedSource;
#[cfg(feature = "pmtiles")]
use martin_core::tiles::pmtiles::PmtCache;
use tracing::{info, instrument, warn};

use super::{Config, ServerState, init_aws_lc_tls, parse_base_path};
#[cfg(feature = "_tiles")]
use super::{ResolutionResult, TileSourceWarning};
use crate::MartinResult;
#[cfg(any(
    feature = "pmtiles",
    feature = "sprites",
    feature = "fonts",
    all(feature = "mlt", feature = "mbtiles"),
))]
use crate::config::file::FileConfigEnum;
#[cfg(all(feature = "mlt", any(feature = "pmtiles", feature = "mbtiles")))]
use crate::config::file::FileConfigSrc;
#[cfg(any(feature = "_tiles", feature = "sprites", feature = "fonts"))]
use crate::config::file::cache::{CacheConfig, SubCacheSetting};
#[cfg(feature = "_tiles")]
use crate::config::file::process::ProcessConfig;
#[cfg(all(feature = "postgres", feature = "mlt"))]
use crate::config::file::process::resolve_process_config;
#[cfg(any(feature = "pmtiles", feature = "mbtiles", feature = "unstable-cog"))]
use crate::config::file::resolve_files;
use crate::config::file::{
    ConfigFileError, ConfigFileResult, ConfigurationLivecycleHooks, UnrecognizedKeys,
    copy_unrecognized_keys_from_config,
};
#[cfg(feature = "_tiles")]
use crate::config::primitives::IdResolver;
#[cfg(feature = "postgres")]
use crate::config::primitives::OptOneMany;
#[cfg(feature = "_tiles")]
use crate::tile_source_manager::TileSourceManager;

impl Config {
    /// Apply defaults to the config, and validate if there is a connection string
    pub fn finalize(&mut self) -> MartinResult<UnrecognizedKeys> {
        let mut res = self.srv.get_unrecognized_keys();
        copy_unrecognized_keys_from_config(&mut res, "", &self.unrecognized);

        #[cfg(all(feature = "mlt", feature = "_tiles"))]
        {
            use crate::config::primitives::AutoOption;
            if let Some(AutoOption::Explicit(cfg)) = self.convert_to_mlt.as_ref() {
                res.extend(
                    cfg.unrecognized_keys()
                        .map(|k| format!("convert_to_mlt.{k}")),
                );
            }
            if let Some(AutoOption::Explicit(cfg)) = self.convert_to_mvt.as_ref() {
                res.extend(
                    cfg.unrecognized_keys()
                        .map(|k| format!("convert_to_mvt.{k}")),
                );
            }
        }

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

    #[instrument(skip_all, err(Debug))]
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

        #[cfg(feature = "_tiles")]
        let tile_sources_with_process = {
            let process_map = self.build_process_config_map();
            let global_process = ProcessConfig::default();
            tile_sources
                .into_iter()
                .map(|group| {
                    group
                        .into_iter()
                        .map(|src| {
                            let pc = process_map
                                .get(src.get_id())
                                .cloned()
                                .unwrap_or_else(|| global_process.clone());
                            (src, pc)
                        })
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>()
        };

        Ok(ServerState {
            #[cfg(feature = "_tiles")]
            tile_manager: TileSourceManager::from_sources(
                cache_config.create_tile_cache(),
                self.on_invalid.unwrap_or_default(),
                tile_sources_with_process,
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
    #[instrument(skip_all, err(Debug))]
    async fn resolve_tile_sources(
        &mut self,
        idr: &IdResolver,
        #[cfg(feature = "pmtiles")] pmtiles_cache: PmtCache,
    ) -> MartinResult<(Vec<Vec<BoxedSource>>, Vec<TileSourceWarning>)> {
        let mut sources_and_warnings: Vec<BoxFuture<ResolutionResult>> = Vec::new();

        #[cfg(feature = "postgres")]
        {
            let config_source = self.source.handle();
            for s in self.postgres.iter_mut() {
                sources_and_warnings.push(Box::pin(s.resolve(
                    idr.clone(),
                    self.cache.policy(),
                    config_source.clone(),
                )));
            }
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

    /// Build a map from source ID -> resolved [`ProcessConfig`].
    ///
    /// Uses full-override semantics: per-source > source-type > global > default.
    #[cfg(feature = "_tiles")]
    fn build_process_config_map(&self) -> HashMap<String, ProcessConfig> {
        #[allow(unused_mut)]
        let mut map = HashMap::new();

        #[cfg(all(
            feature = "mlt",
            any(feature = "postgres", feature = "pmtiles", feature = "mbtiles")
        ))]
        {
            let global = ProcessConfig {
                convert_to_mlt: self.convert_to_mlt.clone(),
                convert_to_mvt: self.convert_to_mvt.clone(),
            };

            #[cfg(feature = "postgres")]
            for pg in self.postgres.iter() {
                let source_type = ProcessConfig {
                    convert_to_mlt: pg.convert_to_mlt.clone(),
                    convert_to_mvt: pg.convert_to_mvt.clone(),
                };
                if let Some(tables) = &pg.tables {
                    for (id, info) in tables {
                        let per_source = ProcessConfig {
                            convert_to_mlt: info.convert_to_mlt.clone(),
                            convert_to_mvt: info.convert_to_mvt.clone(),
                        };
                        map.insert(
                            id.clone(),
                            resolve_process_config(&global, &source_type, &per_source),
                        );
                    }
                }
                if let Some(functions) = &pg.functions {
                    for (id, info) in functions {
                        let per_source = ProcessConfig {
                            convert_to_mlt: info.convert_to_mlt.clone(),
                            convert_to_mvt: info.convert_to_mvt.clone(),
                        };
                        map.insert(
                            id.clone(),
                            resolve_process_config(&global, &source_type, &per_source),
                        );
                    }
                }
            }

            #[cfg(feature = "pmtiles")]
            Self::insert_file_source_configs(&mut map, &global, &self.pmtiles, |c| ProcessConfig {
                convert_to_mlt: c.convert_to_mlt.clone(),
                convert_to_mvt: c.convert_to_mvt.clone(),
            });

            #[cfg(feature = "mbtiles")]
            Self::insert_file_source_configs(&mut map, &global, &self.mbtiles, |c| ProcessConfig {
                convert_to_mlt: c.convert_to_mlt.clone(),
                convert_to_mvt: c.convert_to_mvt.clone(),
            });
        }

        // COG sources produce raster tiles (TIFF), not vector tiles (MVT),
        // so process config (MLT conversion, compression) does not apply.
        // They fall through to the global default, which is a no-op for raster formats.

        map
    }

    /// Helper to resolve process configs for file-based source types (pmtiles, mbtiles).
    #[cfg(all(feature = "mlt", any(feature = "pmtiles", feature = "mbtiles")))]
    fn insert_file_source_configs<T: ConfigurationLivecycleHooks>(
        map: &mut HashMap<String, ProcessConfig>,
        global: &ProcessConfig,
        file_cfg: &FileConfigEnum<T>,
        get_source_type_pc: impl Fn(&T) -> ProcessConfig,
    ) {
        use crate::config::file::process::resolve_process_config;

        if let FileConfigEnum::Config(cfg) = file_cfg {
            let source_type = get_source_type_pc(&cfg.custom);
            if let Some(sources) = &cfg.sources {
                for (id, src) in sources {
                    let per_source = match src {
                        FileConfigSrc::Obj(obj) => ProcessConfig {
                            convert_to_mlt: obj.convert_to_mlt.clone(),
                            convert_to_mvt: obj.convert_to_mvt.clone(),
                        },
                        FileConfigSrc::Path(_) => ProcessConfig::default(),
                    };
                    map.insert(
                        id.clone(),
                        resolve_process_config(global, &source_type, &per_source),
                    );
                }
            }
        }
    }

    pub fn save_to_file(&self, file_name: &Path) -> ConfigFileResult<()> {
        let yaml = serde_yaml::to_string(&self).expect("Unable to serialize config");
        if file_name.as_os_str() == OsStr::new("-") {
            info!("Current system configuration:");
            #[expect(
                clippy::print_stdout,
                reason = "`--save -` writes the config to stdout"
            )]
            {
                println!("\n\n{yaml}\n");
            }
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
