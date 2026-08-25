//! [`PostgresReloader`] writes the `PostgreSQL` connection's sources by
//! - loading them at startup via [`init`](PostgresReloader::init) and
//! - keeps them current by polling.

use std::ops::Add as _;
use std::time::Duration;

use futures::pin_mut;
use tokio::task::JoinHandle;
use tokio::time::timeout;

use crate::TileSourceManager;
use crate::config::args::{BoundsCalcType, DEFAULT_BOUNDS_TIMEOUT};
use crate::config::file::postgres::PostgresConfig;
use crate::config::file::process::ProcessConfig;
#[cfg(all(feature = "mlt", feature = "_tiles"))]
use crate::config::file::resolve_process_config;
use crate::config::file::tiles::discovery::PostgresDiscovery;
use crate::config::file::tiles::driver::{Baseline, PollTrigger, ReloadDriver};
use crate::config::file::{CachePolicy, SourceBuildResult, TileSourceWarning};
use crate::config::primitives::IdResolver;

/// Reloader for `PostgreSQL` sources.
///
/// [`init`](Self::init) publishes everything the catalog discovers into the [`TileSourceManager`] before serving starts.
/// `PostgreSQL` has no change-notification channel Martin could listens to, so [`start`](Self::start) then re-runs discovery on a fixed [`PollTrigger`] interval.
/// The diff (adds, updates, removals) gets applied.
/// A `reload_interval` of `0s` disables polling.
pub struct PostgresReloader {
    driver: ReloadDriver<PostgresDiscovery, TileSourceManager>,
}

impl PostgresReloader {
    /// Resolves the connection-level process config (source-type > global > default) and wires a
    /// [`PostgresDiscovery`] over `config`. The connection pool is built lazily on the first use.
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
            driver: ReloadDriver::new(discovery, tsm),
        }
    }

    /// Publishes every discovered source into the catalog and returns the discovery warnings.
    /// Fails if the database is unreachable or, under `on_invalid: abort`, if any source fails to build.
    pub async fn init(&mut self) -> SourceBuildResult<Vec<TileSourceWarning>> {
        let discovery = self.driver.discovery();
        let auto_bounds = discovery.config().auto_bounds.unwrap_or_default();
        let db = discovery.pool_id().unwrap_or("PostgreSQL").to_owned();
        let init = self.driver.init();
        // warn only if default bounds timeout has already passed
        on_slow(init, DEFAULT_BOUNDS_TIMEOUT.add(Duration::from_secs(1)), || {
            if auto_bounds == BoundsCalcType::Skip {
                tracing::warn!(
                    "Discovering tables in PostgreSQL database '{db}' is taking too long. Bounds calculation is already disabled. You may need to tune your database."
                );
            } else {
                tracing::warn!(
                    "Discovering tables in PostgreSQL database '{db}' is taking too long. Make sure your table geo columns have a GIS index, or use '--auto-bounds skip' CLI/config to skip bbox calculation."
                );
            }
        })
        .await
    }

    /// Spawns the reload driver on the configured poll interval, returning its task handle.
    ///
    /// Returns `None` without spawning when `reload_interval` is `0s`.
    pub fn start(self) -> Option<JoinHandle<()>> {
        let interval = self.driver.discovery().reload_interval();
        if interval.is_zero() {
            tracing::info!("PostgresReloader: runtime reloading disabled (reload_interval = 0s)");
            return None;
        }
        Some(
            self.driver
                .spawn(PollTrigger::new(interval), Baseline::Initialized),
        )
    }
}

async fn on_slow<T, S: FnOnce()>(
    future: impl Future<Output = T>,
    duration: Duration,
    on_slow: S,
) -> T {
    pin_mut!(future);
    if let Ok(result) = timeout(duration, &mut future).await {
        result
    } else {
        on_slow();
        future.await
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
            connection_string: Some("postgres://nope@127.0.0.1:1/none".to_owned()),
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
    #[case::fast(Duration::from_millis(0), false)]
    #[case::slow(Duration::from_millis(50), true)]
    #[tokio::test]
    async fn on_slow_fires_only_when_the_future_outlives_the_deadline(
        #[case] takes: Duration,
        #[case] should_warn: bool,
    ) {
        let warned = std::cell::Cell::new(false);
        let value = super::on_slow(
            async {
                tokio::time::sleep(takes).await;
                42
            },
            Duration::from_millis(10),
            || warned.set(true),
        )
        .await;
        assert_eq!(value, 42, "the future's result is returned either way");
        assert_eq!(warned.get(), should_warn);
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
/// initialized once and then driven one reconcile at a time by a rendezvous [`Trigger`], must
/// mirror CREATE / ALTER / DROP into the [`TileSourceManager`] catalog - including a DROP that
/// lands between startup and the first poll.
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
    use crate::test_support::pg::{
        connection_string, seed, start_postgres_11_with_posgis_3_container,
    };

    /// A [`Trigger`] the test drives in lockstep. Each `next()` first acks that the previous cycle
    /// has finished, then blocks for the test's go-ahead.
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

        /// Blocks until the driver finishes its current cycle.
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

    fn published(tsm: &TileSourceManager, id: &str) -> bool {
        tsm.tile_sources().source_names().contains(&id.to_owned())
    }

    fn has_provenance(tsm: &TileSourceManager, id: &str) -> bool {
        tsm.provenance().iter().any(|(pid, _)| pid == id)
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
    async fn init_then_reload_reflects_create_alter_drop_in_catalog() {
        let container = start_postgres_11_with_posgis_3_container().await;
        let connstr = connection_string(&container).await;

        seed(
            &connstr,
            "CREATE TABLE public.reload_boundary (gid serial PRIMARY KEY, geom geometry(Point, 4326));",
        )
        .await;

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

        let mut driver = ReloadDriver::new(discovery, tsm.clone());
        let warnings = driver.init().await.expect("init");
        assert!(warnings.is_empty(), "unexpected warnings: {warnings:?}");
        assert!(
            published(&tsm, "reload_boundary"),
            "init must publish a table that exists at startup"
        );
        assert!(
            !published(&tsm, "reload_e2e"),
            "must not publish a table that does not exist"
        );
        assert!(has_provenance(&tsm, "reload_boundary"));

        seed(&connstr, "DROP TABLE public.reload_boundary;").await;

        let (trigger, mut rdv) = Rendezvous::new();
        let driver = driver.spawn(trigger, Baseline::Initialized);
        rdv.await_cycle().await;

        rdv.trigger_reconcile().await;
        rdv.await_cycle().await;
        assert!(
            !published(&tsm, "reload_boundary"),
            "a table dropped between init and the first poll must be removed"
        );
        assert!(!has_provenance(&tsm, "reload_boundary"));

        // CREATE -> addition.
        seed(
            &connstr,
            "CREATE TABLE public.reload_e2e (gid serial PRIMARY KEY, geom geometry(Point, 4326));",
        )
        .await;
        rdv.trigger_reconcile().await;
        rdv.await_cycle().await;
        assert!(
            published(&tsm, "reload_e2e"),
            "CREATE TABLE must publish the source"
        );
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
        assert!(
            !published(&tsm, "reload_e2e"),
            "DROP TABLE must remove the source"
        );

        // Dropping the rendezvous closes the tick channel, ending the driver loop.
        drop(rdv);
        driver.await.expect("driver task panicked");
    }
}
