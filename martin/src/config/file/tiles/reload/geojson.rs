use crate::config::file::geojson::GeoJsonConfig;
use crate::config::file::process::ProcessConfig;
use crate::config::file::tiles::discovery::{FsDiscovery, FsSourceBuilder};
use crate::config::file::tiles::driver::{Baseline, NotifyTrigger, ReloadDriver};
use crate::config::file::{FileConfigEnum, TileSourceConfiguration as _};
use crate::config::primitives::IdResolver;
use crate::{MartinResult, TileSourceManager};

/// Watches configured directories for `.json`/`.geojson` changes.
pub struct GeoJsonReloader {
    tile_source_manager: TileSourceManager,
    discovery: FsDiscovery,
}

impl GeoJsonReloader {
    #[must_use]
    pub fn new(
        tsm: TileSourceManager,
        id_resolver: IdResolver,
        config: &FileConfigEnum<GeoJsonConfig>,
    ) -> Self {
        // Discovered files inherit the configured extent and buffer, so the builder closes over the
        // custom config and delegates to its `new_sources` (see `PmtilesReloader::new`).
        let geojson_config = match config {
            FileConfigEnum::Config(cfg) => cfg.custom.clone(),
            _ => GeoJsonConfig::default(),
        };
        let build: FsSourceBuilder = Box::new(move |id, path, policy| {
            let config = geojson_config.clone();
            Box::pin(async move { config.new_sources(id, path, policy).await })
        });
        let discovery = FsDiscovery::from_config(
            config,
            &["json", "geojson"],
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
