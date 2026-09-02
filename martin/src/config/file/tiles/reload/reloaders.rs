#[cfg(feature = "postgres")]
use futures::future::try_join_all;

use crate::StartupResult;
use crate::config::file::Config;
#[cfg(any(feature = "mbtiles", feature = "pmtiles", feature = "postgres"))]
use crate::config::file::process::ProcessConfig;
use crate::config::primitives::IdResolver;
use crate::tile_source_manager::TileSourceManager;

/// Every reloader-owned source kind, constructed together so the binaries share one wiring.
pub struct TileReloaders {
    #[cfg(feature = "mbtiles")]
    mbtiles: super::mbtiles::MbtilesReloader,
    #[cfg(feature = "unstable-cog")]
    cog: super::cog::CogReloader,
    #[cfg(feature = "geojson")]
    geojson: super::geojson::GeoJsonReloader,
    #[cfg(feature = "pmtiles")]
    pmtiles: super::pmtiles::PmtilesReloader,
    #[cfg(feature = "postgres")]
    postgres: Vec<super::postgres::PostgresReloader>,
}

impl TileReloaders {
    /// Constructs every reloader and initializes the PostgreSQL sources.
    /// Nothing is spawned until [`start`](Self::start) is called.
    #[cfg_attr(
        not(feature = "postgres"),
        expect(clippy::unused_async, clippy::unused_async_trait_impl)
    )]
    #[expect(clippy::too_many_lines, reason = "one block per file kind")]
    pub async fn init(
        config: &Config,
        catalog: &TileSourceManager,
        resolver: &IdResolver,
    ) -> StartupResult<Self> {
        #[cfg(any(feature = "mbtiles", feature = "pmtiles", feature = "postgres"))]
        let global_process = {
            #[cfg(feature = "mlt")]
            let pc = ProcessConfig {
                convert_to_mlt: config.convert_to_mlt.clone(),
                convert_to_mvt: config.convert_to_mvt.clone(),
                ..Default::default()
            };
            #[cfg(not(feature = "mlt"))]
            let pc = ProcessConfig::default();
            pc
        };

        #[cfg(feature = "mbtiles")]
        let mut mbtiles = super::mbtiles::MbtilesReloader::new(
            catalog.clone(),
            resolver.clone(),
            &config.mbtiles,
            config.cache.policy(),
            &global_process,
        );
        #[cfg(feature = "unstable-cog")]
        let mut cog = super::cog::CogReloader::new(
            catalog.clone(),
            resolver.clone(),
            &config.cog,
            config.cache.policy(),
        );
        #[cfg(feature = "geojson")]
        let mut geojson = super::geojson::GeoJsonReloader::new(
            catalog.clone(),
            resolver.clone(),
            &config.geojson,
            config.cache.policy(),
        );
        #[cfg(feature = "pmtiles")]
        let mut pmtiles = super::pmtiles::PmtilesReloader::new(
            catalog.clone(),
            resolver.clone(),
            &config.pmtiles,
            config.cache.policy(),
            &global_process,
        );
        #[cfg(feature = "mbtiles")]
        {
            let warnings = mbtiles.init().await?;
            catalog.on_invalid().handle_tile_warnings(&warnings)?;
        }
        #[cfg(feature = "unstable-cog")]
        {
            let warnings = cog.init().await?;
            catalog.on_invalid().handle_tile_warnings(&warnings)?;
        }
        #[cfg(feature = "geojson")]
        {
            let warnings = geojson.init().await?;
            catalog.on_invalid().handle_tile_warnings(&warnings)?;
        }
        #[cfg(feature = "pmtiles")]
        {
            let warnings = pmtiles.init().await?;
            catalog.on_invalid().handle_tile_warnings(&warnings)?;
        }

        #[cfg(feature = "postgres")]
        let postgres = {
            let mut reloaders: Vec<_> = config
                .postgres
                .iter()
                .cloned()
                .map(|pg_config| {
                    super::postgres::PostgresReloader::new(
                        catalog.clone(),
                        resolver.clone(),
                        pg_config,
                        config.cache.policy(),
                        &global_process,
                    )
                })
                .collect();
            let warnings = try_join_all(
                reloaders
                    .iter_mut()
                    .map(super::postgres::PostgresReloader::init),
            )
            .await?;
            catalog
                .on_invalid()
                .handle_tile_warnings(&warnings.into_iter().flatten().collect::<Vec<_>>())?;
            reloaders
        };

        Ok(Self {
            #[cfg(feature = "mbtiles")]
            mbtiles,
            #[cfg(feature = "unstable-cog")]
            cog,
            #[cfg(feature = "geojson")]
            geojson,
            #[cfg(feature = "pmtiles")]
            pmtiles,
            #[cfg(feature = "postgres")]
            postgres,
        })
    }

    /// Spawns every reload loop.
    /// A reloader that cannot start is logged and skipped.
    pub fn start(self) {
        #[cfg(feature = "mbtiles")]
        if let Err(e) = self.mbtiles.start() {
            tracing::warn!("failed to start MbtilesReloader {e:?}");
        }
        #[cfg(feature = "unstable-cog")]
        if let Err(e) = self.cog.start() {
            tracing::warn!("failed to start CogReloader {e:?}");
        }
        #[cfg(feature = "geojson")]
        if let Err(e) = self.geojson.start() {
            tracing::warn!("failed to start GeoJsonReloader {e:?}");
        }
        #[cfg(feature = "pmtiles")]
        if let Err(e) = self.pmtiles.start() {
            tracing::warn!("failed to start PmtilesReloader {e:?}");
        }
        #[cfg(feature = "postgres")]
        for reloader in self.postgres {
            reloader.start();
        }
    }
}
