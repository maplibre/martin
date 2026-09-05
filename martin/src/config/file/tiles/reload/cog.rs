use crate::TileSourceManager;
use crate::config::file::cog::CogConfig;
use crate::config::file::process::ProcessConfig;
use crate::config::file::tiles::discovery::{
    ConfiguredObjectDiscovery, FsDiscovery, FsSourceBuilder, ObjectStoreParser,
    ObjectStoreSourceBuilder,
};
use crate::config::file::tiles::driver::{Baseline, NotifyTrigger, PollTrigger, ReloadDriver};
use crate::config::file::{
    CachePolicy, FileConfigEnum, SourceBuildResult, TileSourceConfiguration as _, TileSourceWarning,
};
use crate::config::primitives::IdResolver;
use crate::reload::FileKind;

/// Watches configured directories for `.tif`/`.tiff` changes, and configured remote objects
/// for replacement.
///
/// Local directories use a [`NotifyTrigger`] for sub-second feedback; configured remote objects
/// (`s3://`, `https://`, …) are re-checked with a `HEAD` request once per
/// [`CogConfig::reload_interval`](CogConfig::reload_interval) because blob stores have no event
/// channel. Each half is its own [`ReloadDriver`] so neither needs a shared mutex.
pub struct CogReloader {
    local: ReloadDriver<FsDiscovery, TileSourceManager>,
    remote: ReloadDriver<ConfiguredObjectDiscovery, TileSourceManager>,
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
        let loaded_versions = cog_config.loaded_remote_versions();
        let local_config = cog_config.clone();
        let build: FsSourceBuilder = Box::new(move |id, path, policy| {
            let config = local_config.clone();
            Box::pin(async move { config.new_sources(id, path, policy).await })
        });
        let discovery = FsDiscovery::from_config(
            FileKind::Cog,
            config,
            cog_config.recursive.unwrap_or_default(),
            &["tif", "tiff"],
            id_resolver.clone(),
            default_cache,
            &ProcessConfig::default(),
            build,
        );
        let parser_config = cog_config.clone();
        let parser: ObjectStoreParser =
            Box::new(move |url| parser_config.object_store.parse_url_opts(url));
        let remote = ConfiguredObjectDiscovery::from_config(
            FileKind::Cog,
            config,
            &["tif", "tiff"],
            "CogReloader",
            cog_config.reload_interval,
            &id_resolver,
            default_cache,
            &ProcessConfig::default(),
            parser,
            ObjectStoreSourceBuilder::Cog(cog_config.for_reload()),
            loaded_versions,
        );
        let loaded_baseline = remote.loaded_baseline();
        Self {
            local: ReloadDriver::new(discovery, tsm.clone()),
            remote: ReloadDriver::new_with_baseline(remote, tsm, loaded_baseline),
        }
    }

    /// Publishes every discovered local source into the catalog and returns the discovery
    /// warnings. Configured remote objects retain the exact versions captured while their startup
    /// sources were opened.
    pub async fn init(&mut self) -> SourceBuildResult<Vec<TileSourceWarning>> {
        self.local.init().await
    }

    /// Spawns the reload drivers. Local discovery starts only with configured directories;
    /// remote replacement detection starts only with configured remote objects.
    pub fn start(self) -> notify::Result<()> {
        let Self { local, remote } = self;

        let directories = local.discovery().directories();
        let recursive = local.discovery().recursive();
        let has_remote = !remote.discovery().is_empty();
        let interval = remote.discovery().reload_interval();

        if !directories.is_empty() {
            let trigger = NotifyTrigger::new(&directories, recursive)?;
            local.spawn(trigger, Baseline::Initialized);
        }

        if has_remote {
            if interval.is_zero() {
                tracing::info!(
                    "CogReloader: remote object polling disabled (reload_interval = 0s)"
                );
            } else {
                let trigger = PollTrigger::after_interval(interval);
                remote.spawn(trigger, Baseline::Initialized);
            }
        }

        Ok(())
    }
}
