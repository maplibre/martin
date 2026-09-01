use crate::TileSourceManager;
use crate::config::file::geojson::GeoJsonConfig;
use crate::config::file::process::ProcessConfig;
use crate::config::file::tiles::discovery::{FsDiscovery, FsSourceBuilder};
use crate::config::file::tiles::driver::{Baseline, NotifyTrigger, ReloadDriver};
use crate::config::file::{
    FileConfigEnum, SourceBuildResult, TileSourceConfiguration as _, TileSourceWarning,
};
use crate::config::primitives::IdResolver;
use crate::reload::FileKind;

/// Watches configured directories for `.json`/`.geojson` changes.
pub struct GeoJsonReloader {
    driver: ReloadDriver<FsDiscovery, TileSourceManager>,
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
            FileConfigEnum::None | FileConfigEnum::Path(_) | FileConfigEnum::Paths(_) => {
                GeoJsonConfig::default()
            }
        };
        let recursive = geojson_config.recursive;
        let build: FsSourceBuilder = Box::new(move |id, path, policy| {
            let config = geojson_config.clone();
            Box::pin(async move { config.new_sources(id, path, policy).await })
        });
        let discovery = FsDiscovery::from_config(
            FileKind::GeoJson,
            config,
            recursive,
            &["json", "geojson"],
            id_resolver,
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
