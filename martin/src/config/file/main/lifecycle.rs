#[cfg(feature = "_tiles")]
use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs::File;
use std::io::prelude::*;
#[cfg(any(feature = "_tiles", feature = "resources"))]
use std::num::NonZeroU64;
use std::path::Path;
#[cfg(any(feature = "_tiles", feature = "resources"))]
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
use crate::StartupResult;
#[cfg(any(feature = "pmtiles", feature = "mbtiles"))]
use crate::config::file::CacheControlHeader;
#[cfg(any(
    feature = "postgres",
    feature = "pmtiles",
    feature = "mbtiles",
    feature = "passthrough",
    feature = "unstable-cog",
    feature = "unstable-duckdb",
    feature = "processing",
    feature = "resources",
    feature = "resources",
    feature = "resources"
))]
use crate::config::file::ConfigurationLivecycleHooks;
#[cfg(feature = "_tiles")]
use crate::config::file::TileGrids;
#[cfg(any(feature = "_tiles", feature = "resources"))]
use crate::config::file::cache::{CacheConfig, SubCacheSetting};
#[cfg(feature = "_tiles")]
use crate::config::file::process::ProcessConfig;
#[cfg(feature = "_tiles")]
use crate::config::file::process::ResolvedProcess;
#[cfg(any(
    feature = "pmtiles",
    feature = "mbtiles",
    feature = "unstable-cog",
    feature = "processing"
))]
use crate::config::file::resolve_files;
use crate::config::file::{CollectUnrecognizedKeys as _, ConfigFileError, ConfigFileResult};
#[cfg(any(
    feature = "pmtiles",
    feature = "mbtiles",
    feature = "unstable-cog",
    feature = "processing"
))]
use crate::config::file::{FileConfig, FileConfigSrc};
#[cfg(feature = "_tiles")]
use crate::config::primitives::IdResolver;
#[cfg(feature = "_tiles")]
use crate::tile_source_manager::TileSourceManager;

#[cfg(feature = "_tiles")]
type ResolvedTileSources = (
    Vec<Vec<BoxedSource>>,
    Vec<TileSourceWarning>,
    HashMap<String, ProcessConfig>,
);

impl Config {
    /// Apply defaults to the config, and validate if there is a connection string
    pub async fn finalize(&mut self) -> StartupResult<()> {
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
        for pg in &mut self.postgres {
            pg.finalize().await?;
        }

        #[cfg(feature = "_tiles")]
        {
            let tile_grids = TileGrids::resolve(&self.tile_grids)?;
            #[cfg(feature = "postgres")]
            for pg in &self.postgres {
                pg.check_tile_grids(&tile_grids)?;
            }
            #[cfg(feature = "mbtiles")]
            TileGrids::check_file_sources(&self.mbtiles, Some(&tile_grids))?;
            #[cfg(feature = "pmtiles")]
            TileGrids::check_file_sources(&self.pmtiles, Some(&tile_grids))?;
            #[cfg(feature = "unstable-cog")]
            TileGrids::check_file_sources(&self.cog, None)?;
            #[cfg(feature = "processing")]
            TileGrids::check_file_sources(&self.geojson, None)?;
            #[cfg(not(any(feature = "postgres", feature = "mbtiles", feature = "pmtiles")))]
            let _ = tile_grids;
        }

        #[cfg(feature = "pmtiles")]
        self.pmtiles.finalize().await?;

        #[cfg(feature = "mbtiles")]
        self.mbtiles.finalize().await?;

        #[cfg(feature = "passthrough")]
        self.passthrough.finalize().await?;

        #[cfg(feature = "unstable-cog")]
        self.cog.finalize().await?;

        #[cfg(feature = "unstable-duckdb")]
        self.duckdb.finalize().await?;

        #[cfg(feature = "processing")]
        self.geojson.finalize().await?;

        #[cfg(feature = "resources")]
        self.sprites.finalize().await?;

        #[cfg(feature = "resources")]
        self.styles.finalize().await?;

        #[cfg(feature = "resources")]
        self.fonts.finalize().await?;

        // Resolving every source's process settings range-checks them; the map itself is
        // rebuilt by `resolve()`.
        #[cfg(feature = "_tiles")]
        self.resolved_process_map()?;

        if self.has_no_sources() {
            Err(ConfigFileError::NoSources.into())
        } else {
            Ok(())
        }
    }

    /// Returns `true` when no source of any enabled kind has been configured.
    fn has_no_sources(&self) -> bool {
        let is_empty = true;

        #[cfg(feature = "postgres")]
        let is_empty = is_empty && self.postgres.is_empty();

        #[cfg(feature = "pmtiles")]
        let is_empty = is_empty && self.pmtiles.is_empty();

        #[cfg(feature = "mbtiles")]
        let is_empty = is_empty && self.mbtiles.is_empty();

        #[cfg(feature = "passthrough")]
        let is_empty = is_empty && self.passthrough.is_empty();

        #[cfg(feature = "unstable-cog")]
        let is_empty = is_empty && self.cog.is_empty();

        #[cfg(feature = "unstable-duckdb")]
        let is_empty = is_empty && self.duckdb.is_empty();

        #[cfg(feature = "processing")]
        let is_empty = is_empty && self.geojson.is_empty();

        #[cfg(feature = "resources")]
        let is_empty = is_empty && self.sprites.is_empty();

        #[cfg(feature = "resources")]
        let is_empty = is_empty && self.styles.is_empty();

        #[cfg(feature = "resources")]
        let is_empty = is_empty && self.fonts.is_empty();

        is_empty
    }

    /// Warn about configuration keys that were not recognized while parsing the config file.
    ///
    /// Call after [`Config::finalize`], which consumes and migrates the keys it recognizes.
    pub fn warn_unrecognized_keys(&self) {
        for key in &self.get_unrecognized_keys() {
            warn!(
                "Ignoring unrecognized configuration key '{key}'. Please check your configuration file for typos."
            );
        }
    }

    #[instrument(skip_all, err(Debug))]
    pub async fn resolve(
        &mut self,
        #[cfg(feature = "_tiles")] idr: &IdResolver,
    ) -> StartupResult<ServerState> {
        init_aws_lc_tls();

        #[cfg(any(feature = "_tiles", feature = "resources"))]
        let cache_config = self.resolve_cache_config();

        #[cfg(feature = "pmtiles")]
        let pmtiles_cache = cache_config.create_pmtiles_cache();

        #[cfg(feature = "_tiles")]
        #[cfg_attr(
            not(feature = "unstable-duckdb"),
            expect(
                unused_variables,
                reason = "only duckdb reports per-source process layers"
            )
        )]
        let (tile_sources, warnings, duckdb_process) = self
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
            #[cfg_attr(
                not(feature = "unstable-duckdb"),
                expect(unused_mut, reason = "duckdb inserts per-source process entries")
            )]
            let mut process_map = self.resolved_process_map()?;

            #[cfg(feature = "unstable-duckdb")]
            self.populate_duckdb_process_map(duckdb_process, &mut process_map)?;

            tile_sources
                .into_iter()
                .map(|group| {
                    group
                        .into_iter()
                        .map(|src| {
                            let pc = process_map.get(src.get_id()).cloned().unwrap_or_default();
                            (src, pc)
                        })
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>()
        };

        #[cfg(feature = "_tiles")]
        let tile_manager = TileSourceManager::from_sources(
            cache_config.create_tile_cache(),
            self.on_invalid.unwrap_or_default(),
            tile_sources_with_process,
        );
        #[cfg(feature = "_tiles")]
        for (alias, sources) in &self.aliases {
            tile_manager
                .tile_sources()
                .add_alias(alias.clone(), sources.clone())
                .map_err(ConfigFileError::TileAliasResolutionFailed)?;
        }

        Ok(ServerState {
            #[cfg(feature = "_tiles")]
            tile_manager,

            #[cfg(feature = "resources")]
            sprites: self.sprites.resolve()?,
            #[cfg(feature = "resources")]
            sprite_cache: cache_config.create_sprite_cache(),

            #[cfg(feature = "resources")]
            fonts: self.fonts.resolve()?,
            #[cfg(feature = "resources")]
            font_cache: cache_config.create_font_cache(),

            #[cfg(feature = "resources")]
            styles: self.styles.resolve()?,
        })
    }

    // cache.size_mb is still respected, but can be overridden by individual cache sizes
    //
    // `cache.size_mb: 0` disables caching, unless overridden by individual cache sizes
    #[cfg(any(feature = "_tiles", feature = "resources"))]
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
                let cache = &self.pmtiles.custom.directory_cache;
                Self::make_sub_cache(
                    cache.size_mb.unwrap_or(cache_size_mb / 4),
                    cache.expiry.or(global_expiry),
                    cache.idle_timeout.or(global_idle),
                )
            };

            #[cfg(feature = "resources")]
            let sprites = {
                let cache = &self.sprites.custom.cache;
                Self::make_sub_cache(
                    cache.size_mb.unwrap_or(cache_size_mb / 8),
                    cache.expiry.or(global_expiry),
                    cache.idle_timeout.or(global_idle),
                )
            };

            #[cfg(feature = "resources")]
            let fonts = {
                let cache = &self.fonts.custom.cache;
                Self::make_sub_cache(
                    cache.size_mb.unwrap_or(cache_size_mb / 8),
                    cache.expiry.or(global_expiry),
                    cache.idle_timeout.or(global_idle),
                )
            };

            CacheConfig {
                #[cfg(feature = "_tiles")]
                tiles,
                #[cfg(feature = "pmtiles")]
                pmtiles,
                #[cfg(feature = "resources")]
                sprites,
                #[cfg(feature = "resources")]
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
                #[cfg(feature = "resources")]
                sprites: Self::make_sub_cache(64, global_expiry, global_idle),
                #[cfg(feature = "resources")]
                fonts: Self::make_sub_cache(64, global_expiry, global_idle),
            }
        }
    }

    /// Helper to create a `SubCacheSetting` from size in MB. Returns `None` if size is 0.
    #[cfg(any(feature = "_tiles", feature = "resources"))]
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
    #[cfg_attr(
        not(any(
            feature = "pmtiles",
            feature = "mbtiles",
            feature = "passthrough",
            feature = "unstable-cog",
            feature = "unstable-duckdb",
            feature = "processing"
        )),
        expect(
            unused_variables,
            reason = "idr is only consumed by file tile backends"
        )
    )]
    async fn resolve_tile_sources(
        &mut self,
        idr: &IdResolver,
        #[cfg(feature = "pmtiles")] pmtiles_cache: PmtCache,
    ) -> StartupResult<ResolvedTileSources> {
        #[cfg(any(feature = "pmtiles", feature = "mbtiles"))]
        let tile_grids = TileGrids::resolve(&self.tile_grids)?;
        #[cfg_attr(
            not(feature = "unstable-duckdb"),
            expect(unused_mut, reason = "duckdb reports per-source process layers here")
        )]
        let mut duckdb_process = HashMap::new();
        #[cfg_attr(
            not(any(
                feature = "pmtiles",
                feature = "mbtiles",
                feature = "passthrough",
                feature = "unstable-cog",
                feature = "unstable-duckdb",
                feature = "processing"
            )),
            expect(unused_mut, reason = "file tile backends push resolved sources here")
        )]
        let mut sources_and_warnings: Vec<BoxFuture<ResolutionResult>> = Vec::new();

        #[cfg(feature = "pmtiles")]
        if !self.pmtiles.is_empty() {
            self.pmtiles.custom.pmtiles_directory_cache = pmtiles_cache;
            let val = resolve_files(
                &mut self.pmtiles,
                idr,
                &["pmtiles"],
                self.cache.policy(),
                Some(&tile_grids),
            );
            sources_and_warnings.push(Box::pin(val));
        }

        #[cfg(feature = "mbtiles")]
        if !self.mbtiles.is_empty() {
            let cfg = &mut self.mbtiles;
            let val = resolve_files(
                cfg,
                idr,
                &["mbtiles"],
                self.cache.policy(),
                Some(&tile_grids),
            );
            sources_and_warnings.push(Box::pin(val));
        }

        #[cfg(feature = "passthrough")]
        if !self.passthrough.is_empty() {
            let val = self.passthrough.resolve(idr, self.cache.policy());
            sources_and_warnings.push(Box::pin(val));
        }

        #[cfg(feature = "unstable-cog")]
        if !self.cog.is_empty() {
            let cfg = &mut self.cog;
            let val = resolve_files(cfg, idr, &["tif", "tiff"], self.cache.policy(), None);
            sources_and_warnings.push(Box::pin(val));
        }

        #[cfg(feature = "unstable-duckdb")]
        if !self.duckdb.is_empty() {
            let val = self.duckdb.resolve(idr.clone(), self.cache.policy());
            let process = &mut duckdb_process;
            sources_and_warnings.push(Box::pin(async move {
                let (sources, warnings, per_source) = val.await?;
                *process = per_source;
                Ok((sources, warnings))
            }));
        }

        #[cfg(feature = "processing")]
        if !self.geojson.is_empty() {
            let cfg = &mut self.geojson;
            let val = resolve_files(cfg, idr, &["json", "geojson"], self.cache.policy(), None);
            sources_and_warnings.push(Box::pin(val));
        }

        let all_results = try_join_all(sources_and_warnings).await?;
        let (all_tile_sources, all_tile_warnings): (Vec<_>, Vec<_>) =
            all_results.into_iter().unzip();

        Ok((
            all_tile_sources,
            all_tile_warnings.into_iter().flatten().collect(),
            duckdb_process,
        ))
    }

    /// The processing settings configured at the top level of the config file, which every source inherits unless it overrides them.
    #[cfg(any(
        feature = "pmtiles",
        feature = "mbtiles",
        feature = "passthrough",
        feature = "unstable-cog",
        feature = "unstable-duckdb",
        feature = "processing"
    ))]
    fn global_process_config(&self) -> ProcessConfig {
        ProcessConfig {
            convert_to_mlt: self.convert_to_mlt.clone(),
            convert_to_mvt: self.convert_to_mvt.clone(),
            // applied by middleware from the server-level default, not carried here
            cache_control: None,
            // `None` since deliberately does not exist at top level
            #[cfg(feature = "processing")]
            convert_to_hillshade: None,
            #[cfg(feature = "processing")]
            convert_to_contour: None,
        }
    }

    /// Source ID -> what it is served with, every level folded and range-checked.
    ///
    /// Uses full-override semantics: per-source > source-type > global > default.
    #[cfg(feature = "_tiles")]
    fn resolved_process_map(&self) -> StartupResult<HashMap<String, ResolvedProcess>> {
        #[allow(unused_mut)]
        let mut map = HashMap::new();

        #[cfg(any(
            feature = "pmtiles",
            feature = "mbtiles",
            feature = "passthrough",
            feature = "unstable-cog",
            feature = "processing"
        ))]
        {
            let global = self.global_process_config();
            #[cfg(any(feature = "pmtiles", feature = "mbtiles"))]
            let archive_cache_control = self
                .srv
                .cache_control
                .is_none()
                .then(CacheControlHeader::archive_default);

            #[cfg(feature = "pmtiles")]
            Self::insert_file_source_configs(&mut map, &global, &self.pmtiles, |c| {
                ProcessConfig {
                    convert_to_mlt: c.convert_to_mlt.clone(),
                    convert_to_mvt: c.convert_to_mvt.clone(),
                    cache_control: archive_cache_control.clone(),
                    #[cfg(feature = "processing")]
                    convert_to_hillshade: None,
                    #[cfg(feature = "processing")]
                    convert_to_contour: None,
                }
            })?;

            #[cfg(feature = "mbtiles")]
            Self::insert_file_source_configs(&mut map, &global, &self.mbtiles, |c| {
                ProcessConfig {
                    convert_to_mlt: c.convert_to_mlt.clone(),
                    convert_to_mvt: c.convert_to_mvt.clone(),
                    cache_control: archive_cache_control.clone(),
                    #[cfg(feature = "processing")]
                    convert_to_hillshade: None,
                    #[cfg(feature = "processing")]
                    convert_to_contour: None,
                }
            })?;

            // COG and GeoJSON have no kind-level conversion settings.
            // COG cannot be hillshaded either: shading reads Mapzen *normal* tiles, whose surface
            // gradients are already per-pixel, whereas a COG holds elevation - which would need
            // metres-per-pixel scaling and its own quantisation handling, a separate feature.
            #[cfg(feature = "unstable-cog")]
            Self::insert_file_source_configs(&mut map, &global, &self.cog, |_| {
                ProcessConfig::default()
            })?;

            #[cfg(feature = "processing")]
            Self::insert_file_source_configs(&mut map, &global, &self.geojson, |_| {
                ProcessConfig::default()
            })?;

            #[cfg(feature = "passthrough")]
            if let Some(sources) = &self.passthrough.sources {
                use crate::config::file::passthrough::PassthroughSrc;

                let source_type = ProcessConfig {
                    convert_to_mlt: self.passthrough.convert_to_mlt.clone(),
                    convert_to_mvt: self.passthrough.convert_to_mvt.clone(),
                    cache_control: None,
                    #[cfg(feature = "processing")]
                    convert_to_hillshade: None,
                    #[cfg(feature = "processing")]
                    convert_to_contour: None,
                };
                Self::insert_source_configs(&mut map, &global, &source_type, sources, |src| {
                    match src {
                        PassthroughSrc::Detailed(obj) => ProcessConfig {
                            convert_to_mlt: obj.convert_to_mlt.clone(),
                            convert_to_mvt: obj.convert_to_mvt.clone(),
                            cache_control: obj.cache_control.clone(),
                            #[cfg(feature = "processing")]
                            convert_to_hillshade: obj.convert_to_hillshade.clone(),
                            #[cfg(all(feature = "processing", feature = "_tiles"))]
                            convert_to_contour: obj.convert_to_contour.clone(),
                        },
                        PassthroughSrc::Shorthand(_) => ProcessConfig::default(),
                    }
                })?;
            }
        }

        Ok(map)
    }

    /// Resolve and insert the effective [`ResolvedProcess`] for each source in a map, layering
    /// per-source settings over the source-type and global defaults.
    #[cfg(any(
        feature = "pmtiles",
        feature = "mbtiles",
        feature = "passthrough",
        feature = "unstable-cog",
        feature = "processing"
    ))]
    fn insert_source_configs<'a, S: 'a>(
        map: &mut HashMap<String, ResolvedProcess>,
        global: &ProcessConfig,
        source_type: &ProcessConfig,
        sources: impl IntoIterator<Item = (&'a String, &'a S)>,
        get_per_source_pc: impl Fn(&S) -> ProcessConfig,
    ) -> StartupResult<()> {
        for (id, src) in sources {
            let resolved = ProcessConfig::layered(global, source_type, &get_per_source_pc(src))
                .resolve()
                .map_err(|e| e.for_source(id.clone()))?;
            map.insert(id.clone(), resolved);
        }
        Ok(())
    }

    /// Helper to resolve process configs for file-based source types.
    #[cfg(any(
        feature = "pmtiles",
        feature = "mbtiles",
        feature = "unstable-cog",
        feature = "processing"
    ))]
    fn insert_file_source_configs<T: ConfigurationLivecycleHooks>(
        map: &mut HashMap<String, ResolvedProcess>,
        global: &ProcessConfig,
        file_cfg: &FileConfig<T>,
        get_source_type_pc: impl Fn(&T) -> ProcessConfig,
    ) -> StartupResult<()> {
        let source_type = get_source_type_pc(&file_cfg.custom);
        Self::insert_source_configs(
            map,
            global,
            &source_type,
            &file_cfg.sources,
            |src| match src {
                FileConfigSrc::Obj(obj) => ProcessConfig {
                    convert_to_mlt: obj.convert_to_mlt.clone(),
                    convert_to_mvt: obj.convert_to_mvt.clone(),
                    cache_control: obj.cache_control.clone(),
                    #[cfg(feature = "processing")]
                    convert_to_hillshade: obj.convert_to_hillshade.clone(),
                    #[cfg(all(feature = "processing", feature = "_tiles"))]
                    convert_to_contour: obj.convert_to_contour.clone(),
                },
                FileConfigSrc::Path(_) => ProcessConfig::default(),
            },
        )
    }

    #[cfg(feature = "unstable-duckdb")]
    fn populate_duckdb_process_map(
        &self,
        per_source: HashMap<String, ProcessConfig>,
        map: &mut HashMap<String, ResolvedProcess>,
    ) -> StartupResult<()> {
        let global = self.global_process_config();
        let source_type = self.duckdb.process_config();
        for (id, per_source) in per_source {
            let resolved = ProcessConfig::layered(&global, &source_type, &per_source)
                .resolve()
                .map_err(|e| e.for_source(id.clone()))?;
            map.insert(id, resolved);
        }
        Ok(())
    }

    /// A copy of this config whose sections carry the sources currently in the catalog:
    /// `postgres` entries get their `tables`/`functions` matched by `connection_string`, and
    /// file-backed sections get an entry per discovered file.
    #[cfg(any(feature = "postgres", feature = "_file_kinds"))]
    #[must_use]
    pub fn with_catalog(&self, catalog: &TileSourceManager) -> Self {
        let mut config = self.clone();
        #[cfg(not(any(feature = "postgres", feature = "_file_kinds")))]
        let _ = catalog;

        #[cfg(feature = "postgres")]
        for pg in &mut config.postgres {
            use crate::config::file::postgres::{FuncInfoSources, SourceSpec, TableInfoSources};
            use crate::reload::SourceProvenance;

            let mut tables = TableInfoSources::new();
            let mut functions = FuncInfoSources::new();
            for (id, provenance) in catalog.provenance() {
                match provenance {
                    SourceProvenance::Postgres {
                        connection_string,
                        spec,
                    } => {
                        if Some(&connection_string) != pg.connection_string.as_ref() {
                            continue;
                        }
                        match *spec {
                            SourceSpec::Table(info) => {
                                tables.insert(id, info);
                            }
                            SourceSpec::Function(info, _) => {
                                functions.insert(id, info);
                            }
                        }
                    }
                    #[cfg(feature = "_file_kinds")]
                    SourceProvenance::File { .. } => {}
                }
            }
            pg.tables = Some(tables);
            pg.functions = Some(functions);
        }

        #[cfg(feature = "_file_kinds")]
        for (id, provenance) in catalog.provenance() {
            use crate::reload::{FileKind, SourceProvenance};

            match provenance {
                SourceProvenance::File { kind, src } => match kind {
                    #[cfg(feature = "mbtiles")]
                    FileKind::Mbtiles => {
                        config.mbtiles.sources.insert(id, src);
                    }
                    #[cfg(feature = "pmtiles")]
                    FileKind::Pmtiles => {
                        config.pmtiles.sources.insert(id, src);
                    }
                    #[cfg(feature = "unstable-cog")]
                    FileKind::Cog => {
                        config.cog.sources.insert(id, src);
                    }
                    #[cfg(feature = "processing")]
                    FileKind::GeoJson => {
                        config.geojson.sources.insert(id, src);
                    }
                },
                #[cfg(feature = "postgres")]
                SourceProvenance::Postgres { .. } => {}
            }
        }

        config
    }

    /// Writes the running configuration, with the tile sources materialized from the catalog so
    /// the file describes what is actually served.
    #[expect(
        clippy::print_stdout,
        reason = "`--save -` writes the config to stdout"
    )]
    pub fn save_to_file(
        &self,
        file_name: &Path,
        #[cfg(feature = "_tiles")] catalog: &TileSourceManager,
    ) -> ConfigFileResult<()> {
        #[cfg(any(feature = "postgres", feature = "_file_kinds"))]
        let config = self.with_catalog(catalog);
        #[cfg(all(
            feature = "_tiles",
            not(any(feature = "postgres", feature = "_file_kinds"))
        ))]
        let _ = catalog;
        #[cfg(not(any(feature = "postgres", feature = "_file_kinds")))]
        let config = self;
        let yaml = serde_saphyr::to_string(&config).expect("Unable to serialize config");
        if file_name.as_os_str() == OsStr::new("-") {
            info!("Current system configuration:");
            println!("\n\n{yaml}\n");
        } else {
            info!(
                "Saving config to {}, use --config to load it",
                file_name.display()
            );
            File::create(file_name)
                .map_err(|e| ConfigFileError::ConfigWriteError(e, file_name.to_path_buf()))?
                .write_all(yaml.as_bytes())
                .map_err(|e| ConfigFileError::ConfigWriteError(e, file_name.to_path_buf()))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::config::test_helpers::render_finalize_failure;

    #[cfg(all(feature = "processing", feature = "passthrough"))]
    #[tokio::test]
    async fn finalize_rejects_an_out_of_range_hillshade() {
        insta::assert_snapshot!(
            render_finalize_failure(indoc::indoc! {"
                passthrough:
                  sources:
                    terrain:
                      url: https://example.org/normal/{z}/{x}/{y}.png
                      convert_to_hillshade:
                        azimuth: 400
            "})
            .await,
            @"Source terrain has an invalid hillshade configuration: Hillshade parameter azimuth must be between `0` and `360`, but was `400`"
        );
    }

    #[cfg(all(feature = "processing", feature = "passthrough"))]
    #[tokio::test]
    async fn hillshade_cannot_be_configured_globally() {
        use crate::config::file::CollectUnrecognizedKeys as _;

        let config: super::Config = serde_saphyr::from_str(indoc::indoc! {"
            convert_to_hillshade: auto
            passthrough:
              sources:
                terrain: https://example.org/normal/{z}/{x}/{y}.png
        "})
        .expect("parses, with the stray key collected rather than rejected");

        let keys = config.get_unrecognized_keys();
        let keys = keys.iter().collect::<Vec<_>>();
        assert_eq!(keys.as_slice(), ["convert_to_hillshade"]);
    }

    #[cfg(all(feature = "processing", feature = "passthrough"))]
    #[tokio::test]
    async fn finalize_accepts_a_valid_hillshade() {
        let mut config: super::Config = serde_saphyr::from_str(indoc::indoc! {"
            passthrough:
              sources:
                terrain:
                  url: https://example.org/normal/{z}/{x}/{y}.png
                  convert_to_hillshade:
                    azimuth: 315
                    format: webp
        "})
        .expect("parses");
        config
            .finalize()
            .await
            .expect("valid hillshade must start up");
    }

    #[tokio::test]
    async fn finalize_no_sources() {
        insta::assert_snapshot!(
            render_finalize_failure("keep_alive: 75\n").await,
            @"No tile sources found. Set sources by giving a database connection string on command line or a config file."
        );
    }
}
