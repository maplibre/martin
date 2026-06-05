use martin_core::tiles::BoxedSource;
use martin_core::tiles::mbtiles::MbtSource;

use crate::config::file::FileConfigEnum;
use crate::config::file::mbtiles::MbtConfig;
use crate::config::file::process::ProcessConfig;
#[cfg(all(feature = "mlt", feature = "_tiles"))]
use crate::config::file::resolve_process_config;
use crate::config::file::tiles::discovery::{FsDiscovery, FsSourceBuilder};
use crate::config::file::tiles::driver::{Baseline, NotifyTrigger, ReloadDriver};
use crate::config::primitives::IdResolver;
use crate::{MartinResult, TileSourceManager};

/// Watches configured directories for `.mbtiles` changes.
pub struct MbtilesReloader {
    tile_source_manager: TileSourceManager,
    discovery: FsDiscovery,
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
        #[cfg(all(feature = "mlt", feature = "_tiles"))]
        let process = {
            let source_type = match config {
                FileConfigEnum::Config(cfg) => ProcessConfig {
                    convert_to_mlt: cfg.custom.convert_to_mlt.clone(),
                    convert_to_mvt: cfg.custom.convert_to_mvt.clone(),
                },
                _ => ProcessConfig::default(),
            };
            resolve_process_config(global_process, &source_type, &ProcessConfig::default())
        };
        #[cfg(not(feature = "mlt"))]
        let process = {
            let _ = (config, global_process);
            ProcessConfig::default()
        };

        // One `FsDiscovery` serves every file kind, so the two boxes erase per-kind types.
        // `Box::pin(async {..})` erases the future to `BoxFuture`.
        // `Box::new(src) as BoxedSource` erases the source to `dyn Source`.
        // The non-capturing closure coerces to the alias's `fn` pointer.
        // The annotation pins the parameter types, which a bare `let` cannot infer.
        let build: FsSourceBuilder = |id, path, policy| {
            Box::pin(async move {
                let src = MbtSource::new(id, path, policy.zoom()).await?;
                Ok(Box::new(src) as BoxedSource)
            })
        };
        let discovery = FsDiscovery::from_config(config, &["mbtiles"], id_resolver, process, build);

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
