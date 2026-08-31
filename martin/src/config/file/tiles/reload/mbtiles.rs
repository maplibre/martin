use martin_core::tiles::BoxedSource;
use martin_core::tiles::mbtiles::MbtSource;

use crate::TileSourceManager;
use crate::config::file::mbtiles::MbtConfig;
use crate::config::file::process::ProcessConfig;
#[cfg(feature = "_process")]
use crate::config::file::resolve_process_config;
use crate::config::file::tiles::discovery::{FsDiscovery, FsSourceBuilder};
use crate::config::file::tiles::driver::{Baseline, NotifyTrigger, ReloadDriver};
use crate::config::file::{FileConfigEnum, SourceBuildResult, TileSourceWarning};
use crate::config::primitives::IdResolver;
use crate::reload::FileKind;

/// Watches configured directories for `.mbtiles` changes.
pub struct MbtilesReloader {
    driver: ReloadDriver<FsDiscovery, TileSourceManager>,
}

impl MbtilesReloader {
    /// Resolves the process config (source-type > global > default) for discovered sources.
    #[must_use]
    pub fn new(
        tsm: TileSourceManager,
        id_resolver: IdResolver,
        config: &FileConfigEnum<MbtConfig>,
        global_process: &ProcessConfig,
    ) -> Self {
        #[cfg(feature = "_process")]
        let process = {
            let source_type = match config {
                FileConfigEnum::Config(cfg) => ProcessConfig {
                    #[cfg(feature = "mlt")]
                    convert_to_mlt: cfg.custom.convert_to_mlt.clone(),
                    #[cfg(feature = "mlt")]
                    convert_to_mvt: cfg.custom.convert_to_mvt.clone(),
                    #[cfg(feature = "hillshade")]
                    convert_to_hillshade: cfg.custom.convert_to_hillshade.clone(),
                    ..Default::default()
                },
                FileConfigEnum::None | FileConfigEnum::Path(_) | FileConfigEnum::Paths(_) => {
                    ProcessConfig::default()
                }
            };
            resolve_process_config(global_process, &source_type, &ProcessConfig::default())
        };
        #[cfg(not(feature = "_process"))]
        let process = {
            let _ = (config, global_process);
            ProcessConfig::default()
        };

        // One `FsDiscovery` serves every file kind, so the two boxes erase per-kind types.
        // `Box::pin(async {..})` erases the future to `BoxFuture`.
        // `Box::new(src) as BoxedSource` erases the source to `dyn Source`.
        // This builder captures nothing.
        // We still `Box::new` it because `FsSourceBuilder` is a boxed `dyn Fn` that `PMTiles` needs (see its docs).
        let build: FsSourceBuilder = Box::new(|id, path, policy| {
            Box::pin(async move {
                let src = MbtSource::new(id, path, policy.zoom()).await?;
                Ok(Box::new(src) as BoxedSource)
            })
        });
        let discovery = FsDiscovery::from_config(
            FileKind::Mbtiles,
            config,
            &["mbtiles"],
            id_resolver,
            process,
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
        if directories.is_empty() {
            return Ok(());
        }
        let trigger = NotifyTrigger::new(&directories)?;
        self.driver.spawn(trigger, Baseline::Initialized);
        Ok(())
    }
}
