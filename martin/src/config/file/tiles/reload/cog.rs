use martin_core::tiles::BoxedSource;
use martin_core::tiles::cog::CogSource;

use crate::TileSourceManager;
use crate::config::file::cog::CogConfig;
use crate::config::file::process::ProcessConfig;
use crate::config::file::tiles::discovery::{FsDiscovery, FsSourceBuilder};
use crate::config::file::tiles::driver::{Baseline, NotifyTrigger, ReloadDriver};
use crate::config::file::{CachePolicy, FileConfigEnum, SourceBuildResult, TileSourceWarning};
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
        // See `MbtilesReloader::new`: both boxes erase per-kind types to a shared shape.
        // This builder captures nothing, but is `Box::new`d to share the boxed `FsSourceBuilder` type.
        let build: FsSourceBuilder = Box::new(|id, path, policy| {
            Box::pin(async move {
                let src = CogSource::new(id, path, policy.zoom())?;
                Ok(Box::new(src) as BoxedSource)
            })
        });
        let recursive = matches!(config, FileConfigEnum::Config(cfg) if cfg.custom.recursive.unwrap_or_default());
        let discovery = FsDiscovery::from_config(
            FileKind::Cog,
            config,
            recursive,
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
