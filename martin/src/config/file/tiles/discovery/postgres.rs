//! [`PostgresDiscovery`]: a [`Discovery`] over a `PostgreSQL` connection's tables and functions.

use std::collections::BTreeMap;
use std::time::Duration;

use martin_core::tiles::BoxedSource;
use tokio::sync::OnceCell;

use crate::config::file::postgres::{PostgresAutoDiscoveryBuilder, PostgresConfig, SourceSpec};
use crate::config::file::tiles::discovery::{Discovery, Version};
use crate::config::file::{CachePolicy, ProcessConfig};
use crate::config::primitives::IdResolver;
use crate::{MartinError, MartinResult};

/// A [`Discovery`] over one `PostgreSQL` connection.
/// 
////Entries are versioned by their [`SourceSpec::fingerprint`], so an in-place
/// data or function-body change (which the fingerprint ignores) does not force a rebuild.
/// The builder owns its own connection pool, created lazily on the first `discover` and reused
/// for the lifetime of the discovery.
pub struct PostgresDiscovery {
    config: PostgresConfig,
    id_resolver: IdResolver,
    default_cache: CachePolicy,
    process: ProcessConfig,
    builder: OnceCell<PostgresAutoDiscoveryBuilder>,
}

impl PostgresDiscovery {
    /// Captures the inputs discovery re-derives from; the connection pool is built lazily.
    #[must_use]
    pub fn new(
        config: PostgresConfig,
        id_resolver: IdResolver,
        default_cache: CachePolicy,
        process: ProcessConfig,
    ) -> Self {
        Self {
            config,
            id_resolver,
            default_cache,
            process,
            builder: OnceCell::new(),
        }
    }

    /// Polling cadence for re-running discovery
    /// `0s` disables reloading.
    #[must_use]
    pub fn reload_interval(&self) -> Duration {
        self.config.reload_interval
    }

    /// The builder, created on first use. A bad connection string surfaces here as an `Err`,
    /// which the driver treats like any other discovery failure (retain the baseline, retry).
    async fn builder(&self) -> MartinResult<&PostgresAutoDiscoveryBuilder> {
        self.builder
            .get_or_try_init(|| async {
                PostgresAutoDiscoveryBuilder::new(
                    &self.config,
                    self.id_resolver.clone(),
                    self.default_cache,
                )
                .await
                .map_err(MartinError::from)
            })
            .await
    }
}

impl Discovery for PostgresDiscovery {
    type Args = SourceSpec;

    async fn discover(&self) -> MartinResult<BTreeMap<String, (Version, Self::Args)>> {
        let (specs, warnings) = self.builder().await?.discover().await?;
        for warning in &warnings {
            tracing::warn!(?warning, "tile source discovery warning during reload");
        }
        Ok(specs
            .into_iter()
            .map(|(id, spec)| (id, (Version::Tracked(spec.fingerprint()), spec)))
            .collect())
    }

    async fn build(&self, id: &str, args: &Self::Args) -> MartinResult<BoxedSource> {
        let (source, _spec) = self.builder().await?.instantiate(id, args.clone()).await?;
        Ok(source)
    }

    fn process(&self) -> ProcessConfig {
        self.process.clone()
    }
}

#[cfg(all(test, feature = "test-pg"))]
mod tests {
    use std::collections::BTreeMap;

    use crate::config::file::CachePolicy;
    use crate::config::file::discovery::{Discovery as _, PostgresDiscovery, Version};
    use crate::config::file::postgres::{PostgresConfig, SourceSpec};
    use crate::config::file::process::ProcessConfig;
    use crate::config::primitives::IdResolver;
    use crate::test_support::pg::{builder_for, seed};

    const TILE_FUNCTION_SQL: &str = "CREATE FUNCTION public.my_func(z integer, x integer, y integer) \
         RETURNS bytea AS $$ SELECT NULL::bytea $$ LANGUAGE sql IMMUTABLE STRICT PARALLEL SAFE;";

    fn discovery_for(connection_string: &str) -> PostgresDiscovery {
        let config = PostgresConfig {
            connection_string: Some(connection_string.to_string()),
            ..Default::default()
        };
        PostgresDiscovery::new(
            config,
            IdResolver::default(),
            CachePolicy::default(),
            ProcessConfig::default(),
        )
    }

    #[tokio::test]
    async fn discover_versions_each_id_by_fingerprint() {
        let (builder, _container, connstr) = builder_for("{}").await;
        seed(
            &connstr,
            "CREATE TABLE public.roads (gid serial PRIMARY KEY, geom geometry(LineString, 4326));",
        )
        .await;
        seed(&connstr, TILE_FUNCTION_SQL).await;

        // The builder is the authority for which ids exist and what each fingerprints to.
        let (specs, _warnings) = builder.discover().await.expect("builder discover");

        let snapshot = discovery_for(&connstr)
            .discover()
            .await
            .expect("discovery discover");

        let snapshot_ids: Vec<&String> = snapshot.keys().collect();
        let spec_ids: Vec<&String> = specs.keys().collect();
        assert_eq!(
            snapshot_ids, spec_ids,
            "discovery ids must match the builder"
        );

        for (id, (version, _args)) in &snapshot {
            assert_eq!(
                *version,
                Version::Tracked(specs[id].fingerprint()),
                "version for {id} must be the spec fingerprint"
            );
        }
    }

    fn versions(snapshot: &BTreeMap<String, (Version, SourceSpec)>) -> BTreeMap<String, Version> {
        snapshot
            .iter()
            .map(|(id, (v, _))| (id.clone(), *v))
            .collect()
    }

    const ROADS_TABLE_SQL: &str =
        "CREATE TABLE public.roads (gid serial PRIMARY KEY, geom geometry(LineString, 4326));";

    #[tokio::test]
    async fn idle_rediscover_is_version_stable() {
        let (_builder, _container, connstr) = builder_for("{}").await;
        seed(&connstr, ROADS_TABLE_SQL).await;

        let discovery = discovery_for(&connstr);
        let first = discovery.discover().await.expect("first discover");
        let second = discovery.discover().await.expect("second discover");
        assert_eq!(
            versions(&first),
            versions(&second),
            "an idle re-discover must report identical versions, so the driver sees no change"
        );
    }

    #[tokio::test]
    async fn schema_change_flips_source_version() {
        let (_builder, _container, connstr) = builder_for("{}").await;
        seed(&connstr, ROADS_TABLE_SQL).await;

        let discovery = discovery_for(&connstr);
        let before = discovery.discover().await.expect("discover before ALTER");

        seed(&connstr, "ALTER TABLE public.roads ADD COLUMN name text;").await;
        let after = discovery.discover().await.expect("discover after ALTER");

        assert_ne!(
            before["roads"].0, after["roads"].0,
            "adding a column must change the source's version so the driver rebuilds it"
        );
    }

    #[tokio::test]
    async fn build_yields_source_with_requested_id() {
        let (_builder, _container, connstr) = builder_for("{}").await;
        seed(
            &connstr,
            "CREATE TABLE public.points (gid serial PRIMARY KEY, geom geometry(Point, 4326));\
             INSERT INTO public.points (geom) VALUES (ST_SetSRID(ST_MakePoint(1, 2), 4326));",
        )
        .await;

        let discovery = discovery_for(&connstr);
        let snapshot = discovery.discover().await.expect("discover");
        let (_version, spec) = snapshot.get("points").expect("spec for points");

        let source = discovery.build("points", spec).await.expect("build");
        assert_eq!(source.get_id(), "points");
    }

    #[tokio::test]
    async fn discover_with_bad_connection_string_errors() {
        // No container: a refused connection must surface as Err on the driver's error path,
        // never a panic.
        let discovery = discovery_for(
            "postgres://nope:nope@127.0.0.1:1/none?connect_timeout=1&sslmode=disable",
        );
        assert!(
            discovery.discover().await.is_err(),
            "a bad connection string must surface as Err, not panic"
        );
    }
}
