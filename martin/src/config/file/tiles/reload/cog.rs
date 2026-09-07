use crate::TileSourceManager;
use crate::config::file::cog::CogConfig;
use crate::config::file::process::ProcessConfig;
use crate::config::file::tiles::discovery::{
    ConfiguredObjectDiscovery, FsDiscovery, FsSourceBuilder, ObjectStoreDiscovery,
    ObjectStoreParser, ObjectStoreSourceBuilder,
};
use crate::config::file::tiles::driver::{Baseline, NotifyTrigger, PollTrigger, ReloadDriver};
use crate::config::file::{
    CachePolicy, FileConfigEnum, SourceBuildResult, TileSourceConfiguration as _, TileSourceWarning,
};
use crate::config::primitives::IdResolver;
use crate::reload::FileKind;

/// Watches configured directories for `.tif`/`.tiff` changes, configured remote objects for
/// replacement, and remote prefixes for additions, replacements, and removals.
///
/// Local directories use a [`NotifyTrigger`] for sub-second feedback. Configured remote objects
/// and prefixes (`s3://`, `https://`, …) use [`PollTrigger`]s because blob stores have no event
/// channel. Each source group has its own [`ReloadDriver`] so none needs a shared mutex.
pub struct CogReloader {
    local: ReloadDriver<FsDiscovery, TileSourceManager>,
    configured: ReloadDriver<ConfiguredObjectDiscovery, TileSourceManager>,
    prefixes: ReloadDriver<ObjectStoreDiscovery, TileSourceManager>,
}

impl CogReloader {
    #[must_use]
    pub fn new(
        tsm: TileSourceManager,
        id_resolver: IdResolver,
        config: &FileConfigEnum<CogConfig>,
        default_cache: CachePolicy,
    ) -> Self {
        let default_cache = config.cache_or(default_cache);
        let cog_config = match config {
            FileConfigEnum::Config(cfg) => cfg.custom.clone(),
            FileConfigEnum::None | FileConfigEnum::Path(_) | FileConfigEnum::Paths(_) => {
                CogConfig::default()
            }
        };
        let local_config = cog_config.clone();
        let build: FsSourceBuilder = Box::new(move |id, path, policy| {
            let config = local_config.clone();
            Box::pin(async move { config.new_sources(id, path, policy).await })
        });
        let local = FsDiscovery::from_config(
            FileKind::Cog,
            config,
            cog_config.recursive.unwrap_or_default(),
            &["tif", "tiff"],
            id_resolver.clone(),
            default_cache,
            &ProcessConfig::default(),
            build,
        );
        let configured_parser_config = cog_config.clone();
        let configured_parser: ObjectStoreParser =
            Box::new(move |url| configured_parser_config.object_store.parse_url_opts(url));
        let configured = ConfiguredObjectDiscovery::from_config(
            FileKind::Cog,
            config,
            "CogReloader",
            cog_config.reload_interval,
            default_cache,
            &ProcessConfig::default(),
            configured_parser,
            ObjectStoreSourceBuilder::Cog(Box::new(cog_config.clone())),
        );
        let prefix_parser_config = cog_config.clone();
        let prefix_parser: ObjectStoreParser =
            Box::new(move |url| prefix_parser_config.object_store.parse_url_opts(url));
        let prefixes = ObjectStoreDiscovery::from_config(
            config,
            &["tif", "tiff"],
            "CogReloader",
            cog_config.reload_interval,
            id_resolver,
            default_cache,
            &ProcessConfig::default(),
            prefix_parser,
            ObjectStoreSourceBuilder::Cog(Box::new(cog_config)),
        );
        Self {
            local: ReloadDriver::new(local, tsm.clone()),
            configured: ReloadDriver::new(configured, tsm.clone()),
            prefixes: ReloadDriver::new(prefixes, tsm),
        }
    }

    /// Publishes every discovered local source into the catalog and returns the discovery warnings.
    /// Configured remote sources were already loaded by startup resolution; remote prefixes are
    /// first listed when their polling driver starts.
    pub async fn init(&mut self) -> SourceBuildResult<Vec<TileSourceWarning>> {
        self.local.init().await
    }

    /// Spawns the reload drivers that have configured inputs.
    pub fn start(self) -> notify::Result<()> {
        let Self {
            local,
            configured,
            prefixes,
        } = self;

        let directories = local.discovery().directories();
        let recursive = local.discovery().recursive();
        let has_configured = !configured.discovery().is_empty();
        let has_prefixes = !prefixes.discovery().remote_prefixes().is_empty();
        let interval = configured.discovery().reload_interval();
        debug_assert_eq!(interval, prefixes.discovery().reload_interval());

        if !directories.is_empty() {
            let trigger = NotifyTrigger::new(&directories, recursive)?;
            local.spawn(trigger, Baseline::Initialized);
        }

        if has_configured || has_prefixes {
            if interval.is_zero() {
                tracing::info!(
                    "CogReloader: remote object and prefix polling disabled (reload_interval = 0s)"
                );
            } else {
                if has_configured {
                    configured.spawn(
                        PollTrigger::after_interval(interval),
                        Baseline::StartupResolved,
                    );
                }
                if has_prefixes {
                    prefixes.spawn(PollTrigger::new(interval), Baseline::Empty);
                }
            }
        }

        Ok(())
    }
}
