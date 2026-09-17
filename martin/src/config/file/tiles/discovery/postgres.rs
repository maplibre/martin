//! [`PostgresDiscovery`]: a [`Discovery`] over a `PostgreSQL` connection's tables and functions.

use std::time::Duration;

use tokio::sync::OnceCell;

use crate::config::file::postgres::{PostgresAutoDiscoveryBuilder, PostgresConfig, SourceSpec};
use crate::config::file::tiles::discovery::{BuiltSource, Discovered, Discovery, Version};
use crate::config::file::{
    CachePolicy, ProcessConfig, ProcessResolveError, ResolvedProcess, SourceBuildError,
    SourceBuildResult,
};
use crate::config::primitives::IdResolver;
use crate::reload::SourceProvenance;

/// A [`Discovery`] over one `PostgreSQL` connection.
///
/// Entries are versioned by their [`SourceSpec::fingerprint`] (hash over fields that affect served tile bytes or metadata).
/// An in-place data or function-body change (which the fingerprint ignores) does not force a rebuild.
/// The builder owns its own connection pool, created lazily on the first `discover` and reused for the lifetime of the discovery.
pub struct PostgresDiscovery {
    config: PostgresConfig,
    id_resolver: IdResolver,
    default_cache: CachePolicy,
    /// The connection level, which per-source settings layer over.
    process: ProcessConfig,
    /// The connection level resolved, for every source without its own settings.
    resolved: ResolvedProcess,
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
            resolved: process
                .resolve()
                .expect("the connection level carries no range-checked settings"),
            process,
            builder: OnceCell::new(),
        }
    }

    /// Polling cadence for re-running discovery
    /// `0s` disables reloading.
    #[must_use]
    pub const fn reload_interval(&self) -> Duration {
        self.config.reload_interval
    }

    #[must_use]
    pub const fn config(&self) -> &PostgresConfig {
        &self.config
    }

    /// The pool id (database name), once the first `discover` has connected.
    #[must_use]
    pub fn pool_id(&self) -> Option<&str> {
        self.builder.get().map(PostgresAutoDiscoveryBuilder::get_id)
    }

    /// The builder, created on first use. A bad connection string surfaces here as an `Err`,
    /// which the driver treats like any other discovery failure (retain the baseline, retry).
    async fn builder(&self) -> SourceBuildResult<&PostgresAutoDiscoveryBuilder> {
        self.builder
            .get_or_try_init(|| async {
                PostgresAutoDiscoveryBuilder::new(
                    &self.config,
                    self.id_resolver.clone(),
                    self.default_cache,
                )
                .await
                .map_err(SourceBuildError::from)
            })
            .await
    }
}

impl Discovery for PostgresDiscovery {
    type Args = SourceSpec;

    async fn discover(&self) -> SourceBuildResult<Discovered<Self::Args>> {
        let (specs, warnings) = self.builder().await?.discover().await?;
        Ok(Discovered {
            sources: specs
                .into_iter()
                .map(|(id, spec)| (id, (Version::Tracked(spec.fingerprint()), spec)))
                .collect(),
            warnings,
        })
    }

    async fn build(&self, id: &str, args: &Self::Args) -> SourceBuildResult<BuiltSource> {
        let (source, spec) = self.builder().await?.instantiate(id, args.clone()).await?;
        log_published(id, &spec);
        Ok(BuiltSource {
            source,
            process: Some(
                per_source_process(&self.process, &spec)
                    .map_err(|e| e.for_source(id.to_owned()))?,
            ),
            provenance: Some(SourceProvenance::Postgres {
                connection_string: self
                    .config
                    .connection_string
                    .clone()
                    .expect("connection_string is set after PostgresConfig::finalize()"),
                spec: Box::new(spec),
            }),
        })
    }

    fn process(&self) -> ResolvedProcess {
        self.resolved.clone()
    }
}

/// Per-source `convert_to_*` and `cache_control` settings layered over the connection level.
fn per_source_process(
    connection: &ProcessConfig,
    spec: &SourceSpec,
) -> Result<ResolvedProcess, ProcessResolveError> {
    let per_source = match spec {
        SourceSpec::Table(info) => ProcessConfig {
            #[cfg(all(feature = "mlt", feature = "_tiles"))]
            convert_to_mlt: info.convert_to_mlt.clone(),
            #[cfg(all(feature = "mlt", feature = "_tiles"))]
            convert_to_mvt: info.convert_to_mvt.clone(),
            cache_control: info.cache_control.clone(),
            #[cfg(feature = "hillshade")]
            convert_to_hillshade: None,
            #[cfg(feature = "contour")]
            convert_to_contour: None,
        },
        SourceSpec::Function(info, _) => ProcessConfig {
            #[cfg(all(feature = "mlt", feature = "_tiles"))]
            convert_to_mlt: info.convert_to_mlt.clone(),
            #[cfg(all(feature = "mlt", feature = "_tiles"))]
            convert_to_mvt: info.convert_to_mvt.clone(),
            cache_control: info.cache_control.clone(),
            #[cfg(feature = "hillshade")]
            convert_to_hillshade: None,
            #[cfg(feature = "contour")]
            convert_to_contour: None,
        },
    };
    ProcessConfig::layered(connection, &ProcessConfig::default(), &per_source).resolve()
}

fn log_published(id: &str, spec: &SourceSpec) {
    match spec {
        SourceSpec::Table(info) => {
            let kind = match info.relkind {
                Some('v') => "view",
                Some('m') => "materialized view",
                _ => "table",
            };
            tracing::info!(
                source.id = %id,
                source.kind = kind,
                schema = %info.schema,
                table = %info.table,
                geometry_column = %info.geometry_column,
                geometry_type = info.geometry_type.as_deref().unwrap_or("unknown"),
                srid = info.srid,
                id_column = info.id_column.as_deref().unwrap_or("none"),
                "Published source"
            );
        }
        SourceSpec::Function(info, sql) => {
            tracing::info!(
                source.id = %id,
                source.kind = "function",
                schema = %info.schema,
                function = %info.function,
                function.signature = %sql.signature,
                "Published source"
            );
        }
    }
}

#[cfg(all(test, feature = "test-pg"))]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::config::file::CachePolicy;
    use crate::config::file::discovery::{PostgresDiscovery, Version};
    use crate::config::file::postgres::{PostgresConfig, SourceSpec};
    use crate::config::file::process::ProcessConfig;
    use crate::config::primitives::IdResolver;
    use crate::test_support::pg::{builder_for, seed};

    const TILE_FUNCTION_SQL: &str = "CREATE FUNCTION public.my_func(z integer, x integer, y integer) \
         RETURNS bytea AS $$ SELECT NULL::bytea $$ LANGUAGE sql IMMUTABLE STRICT PARALLEL SAFE;";

    fn discovery_for(connection_string: &str) -> PostgresDiscovery {
        let config = PostgresConfig {
            connection_string: Some(connection_string.to_owned()),
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
            .expect("discovery discover")
            .sources;

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
        let first = discovery.discover().await.expect("first discover").sources;
        let second = discovery.discover().await.expect("second discover").sources;
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
        let before = discovery
            .discover()
            .await
            .expect("discover before ALTER")
            .sources;

        seed(&connstr, "ALTER TABLE public.roads ADD COLUMN name text;").await;
        let after = discovery
            .discover()
            .await
            .expect("discover after ALTER")
            .sources;

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
        let snapshot = discovery.discover().await.expect("discover").sources;
        let (_version, spec) = snapshot.get("points").expect("spec for points");

        let built = discovery.build("points", spec).await.expect("build");
        assert_eq!(built.source.get_id(), "points");
        assert!(built.provenance.is_some());
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

    #[cfg(feature = "mlt")]
    #[test]
    fn per_source_convert_overrides_the_connection_level() {
        use crate::config::file::postgres::TableInfo;
        use crate::config::file::process::MltConversion;
        use crate::config::primitives::AutoOption;
        let connection = ProcessConfig {
            convert_to_mlt: Some(AutoOption::Auto),
            convert_to_mvt: None,
            cache_control: None,
            #[cfg(feature = "hillshade")]
            convert_to_hillshade: None,
            #[cfg(feature = "contour")]
            convert_to_contour: None,
        };

        let with_override = SourceSpec::Table(TableInfo {
            convert_to_mlt: Some(AutoOption::Disabled),
            ..TableInfo::default()
        });
        assert_eq!(
            per_source_process(&connection, &with_override).unwrap().mlt,
            MltConversion::Disabled,
            "a per-table convert_to_mlt must win over the connection-level setting"
        );

        let without_override = SourceSpec::Table(TableInfo::default());
        assert_eq!(
            per_source_process(&connection, &without_override).unwrap(),
            connection.resolve().unwrap(),
            "a table without overrides must inherit the connection-level setting"
        );
    }
}
