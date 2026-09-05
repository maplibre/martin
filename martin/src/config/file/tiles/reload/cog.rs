use crate::TileSourceManager;
use crate::config::file::cog::CogConfig;
use crate::config::file::process::ProcessConfig;
use crate::config::file::tiles::discovery::{FsDiscovery, FsSourceBuilder};
use crate::config::file::tiles::driver::{Baseline, NotifyTrigger, ReloadDriver};
use crate::config::file::{
    CachePolicy, FileConfigEnum, SourceBuildResult, TileSourceConfiguration as _, TileSourceWarning,
};
use crate::config::primitives::IdResolver;
use crate::reload::FileKind;

/// Watches configured directories for `.tif`/`.tiff` changes.
pub struct CogReloader {
    driver: ReloadDriver<FsDiscovery, TileSourceManager>,
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
        let discovery = FsDiscovery::from_config(
            FileKind::Cog,
            config,
            cog_config.recursive.unwrap_or_default(),
            &["tif", "tiff"],
            id_resolver,
            default_cache,
            &ProcessConfig::default(),
            build,
        );
        Self {
            driver: ReloadDriver::new(discovery, tsm),
        }
    }

    /// Publishes every discovered source into the catalog and returns the discovery warnings.
    pub async fn init(&mut self) -> SourceBuildResult<Vec<TileSourceWarning>> {
        self.driver.init().await
    }

    /// Spawns the reload driver. Does nothing if no directories are configured.
    pub fn start(self) -> notify::Result<()> {
        let directories = self.driver.discovery().directories();
        let recursive = self.driver.discovery().recursive();
        if directories.is_empty() {
            return Ok(());
        }
        let trigger = NotifyTrigger::new(&directories, recursive)?;
        self.driver.spawn(trigger, Baseline::Initialized);
        Ok(())
    }
}
