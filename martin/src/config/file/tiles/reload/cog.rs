use martin_core::tiles::BoxedSource;
use martin_core::tiles::cog::CogSource;

use crate::config::file::FileConfigEnum;
use crate::config::file::cog::CogConfig;
use crate::config::file::driver::Sink as _;
use crate::config::file::process::ProcessConfig;
use crate::config::file::tiles::discovery::{FsDiscovery, FsSourceBuilder};
use crate::config::file::tiles::driver::{Baseline, NotifyTrigger, ReloadDriver};
use crate::config::primitives::IdResolver;
use crate::{MartinResult, TileSourceManager};

/// Watches configured directories for `.tif`/`.tiff` changes.
pub struct CogReloader {
    tile_source_manager: TileSourceManager,
    discovery: FsDiscovery,
}

impl CogReloader {
    #[must_use]
    pub fn new(
        tsm: TileSourceManager,
        id_resolver: IdResolver,
        config: &FileConfigEnum<CogConfig>,
    ) -> Self {
        let build: FsSourceBuilder = Box::new(|id, path, policy| {
            Box::pin(async move {
                let src = CogSource::new(id, path, policy.zoom())?;
                Ok(Box::new(src) as BoxedSource)
            })
        });
        let discovery = FsDiscovery::from_config(
            config,
            &["tif", "tiff"],
            id_resolver,
            ProcessConfig::default(),
            build,
        );
        Self {
            tile_source_manager: tsm,
            discovery,
        }
    }

    /// Spawns the reload driver. Does nothing if no directories are configured.
    pub fn start(self) -> MartinResult<()> {
        let directories = self.discovery.directories().to_vec();
        if directories.is_empty() {
            return Ok(());
        }
        let trigger = NotifyTrigger::new(&directories)?;
        ReloadDriver::new(self.discovery, self.tile_source_manager)
            .spawn(trigger, Baseline::StartupResolved);
        Ok(())
    }
}
