//! [`PostgresReloader`]: polls `PostgreSQL` catalog discovery and applies the diff at runtime.

use tokio::task::JoinHandle;

use crate::TileSourceManager;
use crate::config::file::CachePolicy;
use crate::config::file::postgres::PostgresConfig;
use crate::config::file::process::ProcessConfig;
#[cfg(all(feature = "mlt", feature = "_tiles"))]
use crate::config::file::resolve_process_config;
use crate::config::file::tiles::discovery::PostgresDiscovery;
use crate::config::file::tiles::driver::{Baseline, PollTrigger, ReloadDriver};
use crate::config::primitives::IdResolver;

/// Reloader for `PostgreSQL` sources.
///
/// `PostgreSQL` has no change-notification channel Martin listens to, so this re-runs catalog
/// discovery on a fixed [`PollTrigger`] interval and applies the diff (adds, updates, removals)
/// to the [`TileSourceManager`]. A `reload_interval` of `0s` disables it.
pub struct PostgresReloader {
    tile_source_manager: TileSourceManager,
    discovery: PostgresDiscovery,
}

impl PostgresReloader {
    /// Resolves the connection-level process config (source-type > global > default) and wires a
    /// [`PostgresDiscovery`] over `config`. The connection pool is built lazily on the first poll.
    #[must_use]
    pub fn new(
        tsm: TileSourceManager,
        id_resolver: IdResolver,
        config: PostgresConfig,
        default_cache: CachePolicy,
        global_process: &ProcessConfig,
    ) -> Self {
        #[cfg(all(feature = "mlt", feature = "_tiles"))]
        let process = {
            let source_type = ProcessConfig {
                convert_to_mlt: config.convert_to_mlt.clone(),
                convert_to_mvt: config.convert_to_mvt.clone(),
            };
            resolve_process_config(global_process, &source_type, &ProcessConfig::default())
        };
        #[cfg(not(all(feature = "mlt", feature = "_tiles")))]
        let process = {
            let _ = global_process;
            ProcessConfig::default()
        };

        let discovery = PostgresDiscovery::new(config, id_resolver, default_cache, process);
        Self {
            tile_source_manager: tsm,
            discovery,
        }
    }

    /// Spawns the reload driver on the configured poll interval, returning its task handle.
    ///
    /// Returns `None` without spawning when `reload_interval` is `0s`.
    pub fn start(self) -> Option<JoinHandle<()>> {
        let interval = self.discovery.reload_interval();
        if interval.is_zero() {
            tracing::info!("PostgresReloader: runtime reloading disabled (reload_interval = 0s)");
            return None;
        }
        Some(
            ReloadDriver::new(self.discovery, self.tile_source_manager)
                .spawn(PollTrigger::new(interval), Baseline::StartupResolved),
        )
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use rstest::rstest;

    use crate::TileSourceManager;
    use crate::config::file::postgres::PostgresConfig;
    use crate::config::file::process::ProcessConfig;
    use crate::config::file::reload::postgres::PostgresReloader;
    use crate::config::file::{CachePolicy, OnInvalid};
    use crate::config::primitives::IdResolver;

    fn reloader_with_interval(interval: Duration) -> PostgresReloader {
        let tsm = TileSourceManager::new(None, OnInvalid::Warn);
        let config = PostgresConfig {
            // Never connected to in the disabled case; only reached if a driver actually spawns.
            connection_string: Some("postgres://nope@127.0.0.1:1/none".to_string()),
            reload_interval: interval,
            ..Default::default()
        };
        PostgresReloader::new(
            tsm,
            IdResolver::new(&[]),
            config,
            CachePolicy::default(),
            &ProcessConfig::default(),
        )
    }

    #[rstest]
    #[case::zero_disables(Duration::ZERO, false)]
    #[case::nonzero_spawns(Duration::from_mins(10), true)]
    #[tokio::test]
    async fn start_respects_reload_interval(
        #[case] interval: Duration,
        #[case] should_spawn: bool,
    ) {
        let handle = reloader_with_interval(interval).start();
        assert_eq!(
            handle.is_some(),
            should_spawn,
            "reload_interval {interval:?} must {} a driver task",
            if should_spawn { "spawn" } else { "not spawn" },
        );
        if let Some(handle) = handle {
            handle.abort();
        }
    }
}

/// End-to-end reload against a live container: a real [`ReloadDriver`] + [`PostgresDiscovery`],
/// driven one reconcile at a time by a rendezvous [`Trigger`], must mirror CREATE / ALTER / DROP
/// into the [`TileSourceManager`] catalog.
#[cfg(all(test, feature = "test-pg"))]
mod e2e {
    use std::collections::BTreeMap;

    use tokio::sync::mpsc;

    use crate::TileSourceManager;
    use crate::config::file::postgres::PostgresConfig;
    use crate::config::file::process::ProcessConfig;
    use crate::config::file::tiles::discovery::PostgresDiscovery;
    use crate::config::file::tiles::driver::{Baseline, ReloadDriver, Trigger};
    use crate::config::file::{CachePolicy, OnInvalid};
    use crate::config::primitives::IdResolver;
    use crate::test_support::pg::{connection_string, seed, start_postgres_11_with_posgis_3_container};

    /// A [`Trigger`] the test drives in lockstep. Each `next()` first acks that the previous cycle
    /// (the seed, or a reconcile) has finished, then blocks for the test's go-ahead.
    struct RendezvousTrigger {
        ticks: mpsc::Receiver<()>,
        acks: mpsc::Sender<()>,
    }

    impl Trigger for RendezvousTrigger {
        async fn next(&mut self) -> Option<()> {
            // The ack for the cycle that just finished; ignored once the test drops its handle.
            let _ = self.acks.send(()).await;
            // `None` (test dropped its tick sender) ends the driver loop.
            self.ticks.recv().await
        }
    }

    /// The test side of a [`RendezvousTrigger`].
    struct Rendezvous {
        ticks: mpsc::Sender<()>,
        acks: mpsc::Receiver<()>,
    }

    impl Rendezvous {
        fn new() -> (RendezvousTrigger, Self) {
            let (tick_tx, tick_rx) = mpsc::channel(1);
            let (ack_tx, ack_rx) = mpsc::channel(1);
            (
                RendezvousTrigger {
                    ticks: tick_rx,
                    acks: ack_tx,
                },
                Self {
                    ticks: tick_tx,
                    acks: ack_rx,
                },
            )
        }

        /// Blocks until the driver finishes its current cycle (seed, or a prior reconcile).
        async fn await_cycle(&mut self) {
            self.acks
                .recv()
                .await
                .expect("driver task ended unexpectedly");
        }

        /// Requests exactly one reconcile.
        async fn trigger_reconcile(&self) {
            self.ticks
                .send(())
                .await
                .expect("driver task ended unexpectedly");
        }
    }

    fn published(tsm: &TileSourceManager) -> bool {
        tsm.tile_sources()
            .source_names()
            .contains(&"reload_e2e".to_string())
    }

    /// The fields the published source advertises (its table's non-geometry columns).
    fn advertised_fields(tsm: &TileSourceManager) -> BTreeMap<String, String> {
        let (source, _process) = tsm
            .tile_sources()
            .get_source("reload_e2e")
            .expect("source present");
        source
            .get_tilejson()
            .vector_layers
            .as_ref()
            .and_then(|layers| layers.first())
            .map(|layer| layer.fields.clone())
            .unwrap_or_default()
    }

    #[tokio::test]
    async fn reload_reflects_create_alter_drop_in_catalog() {
        let container = start_postgres_11_with_posgis_3_container().await;
        let connstr = connection_string(&container).await;

        let config = PostgresConfig {
            connection_string: Some(connstr.clone()),
            ..Default::default()
        };
        let discovery = PostgresDiscovery::new(
            config,
            IdResolver::new(&[]),
            CachePolicy::default(),
            ProcessConfig::default(),
        );
        // `Warn`, not `Abort`: under `Abort` one failed source wedges every later tick.
        let tsm = TileSourceManager::new(None, OnInvalid::Warn);

        let (trigger, mut rdv) = Rendezvous::new();
        let driver =
            ReloadDriver::new(discovery, tsm.clone()).spawn(trigger, Baseline::StartupResolved);

        // The seed establishes the baseline; our table does not exist yet.
        rdv.await_cycle().await;
        assert!(
            !published(&tsm),
            "must not publish a table that does not exist"
        );

        // CREATE -> addition.
        seed(
            &connstr,
            "CREATE TABLE public.reload_e2e (gid serial PRIMARY KEY, geom geometry(Point, 4326));",
        )
        .await;
        rdv.trigger_reconcile().await;
        rdv.await_cycle().await;
        assert!(published(&tsm), "CREATE TABLE must publish the source");
        assert!(
            !advertised_fields(&tsm).contains_key("label"),
            "the not-yet-added column must not be advertised"
        );

        // ALTER ADD COLUMN -> update (the published source's advertised fields gain the column).
        seed(
            &connstr,
            "ALTER TABLE public.reload_e2e ADD COLUMN label text;",
        )
        .await;
        rdv.trigger_reconcile().await;
        rdv.await_cycle().await;
        assert!(
            advertised_fields(&tsm).contains_key("label"),
            "ALTER TABLE ADD COLUMN must update the published source"
        );

        // DROP -> removal.
        seed(&connstr, "DROP TABLE public.reload_e2e;").await;
        rdv.trigger_reconcile().await;
        rdv.await_cycle().await;
        assert!(!published(&tsm), "DROP TABLE must remove the source");

        // Dropping the rendezvous closes the tick channel, ending the driver loop.
        drop(rdv);
        driver.await.expect("driver task panicked");
    }
}
