//! Shared `#[cfg(test)]` fixtures for the reload / tile-source machinery.

/// `PostgreSQL` test-container helpers shared by the builder, discovery, and reload tests.
///
/// Each helper spins up its own pinned `PostGIS` container, so tests never touch the shared
/// `just start` database (which ships pre-existing TIGER/public tables and is ANALYZE-sensitive).
#[cfg(feature = "test-pg")]
pub(crate) mod pg {
    use backon::{ConstantBuilder, Retryable as _};
    use martin_core::tiles::postgres::PostgresPool;
    use testcontainers_modules::postgres::Postgres;
    use testcontainers_modules::testcontainers::runners::AsyncRunner as _;
    use testcontainers_modules::testcontainers::{ContainerAsync, ImageExt as _};

    use crate::config::file::CachePolicy;
    use crate::config::file::postgres::{PostgresAutoDiscoveryBuilder, PostgresConfig};
    use crate::config::primitives::IdResolver;

    /// Launches the pinned, purposely-old `PostGIS` image, retrying a few times for flaky CI pulls.
    pub(crate) async fn start_postgres_11_with_posgis_3_container() -> ContainerAsync<Postgres> {
        const MAX_START_ATTEMPTS: usize = 3;
        const RETRY_DELAY: std::time::Duration = std::time::Duration::from_secs(2);

        (|| async {
            Postgres::default()
                .with_name("postgis/postgis")
                .with_tag("11-3.0") // purposely very old and stable
                .start()
                .await
        })
        .retry(
            ConstantBuilder::default()
                .with_delay(RETRY_DELAY)
                .with_max_times(MAX_START_ATTEMPTS),
        )
        .sleep(tokio::time::sleep)
        .await
        .expect("failed to launch container after retry attempts")
    }

    /// The libpq connection string for a running container.
    pub(crate) async fn connection_string(container: &ContainerAsync<Postgres>) -> String {
        let host = container.get_host().await.expect("resolve container host");
        let port = container
            .get_host_port_ipv4(5432)
            .await
            .expect("resolve container port");
        format!("postgres://postgres:postgres@{host}:{port}/postgres?sslmode=disable")
    }

    /// A builder wired to a fresh container from the given [`PostgresConfig`] YAML.
    ///
    /// Returns the connection string too, so a test can [`seed`] through a separate connection
    /// (the builder's own pool is private) or hand it to a [`PostgresDiscovery`].
    ///
    /// [`PostgresDiscovery`]: crate::config::file::discovery::PostgresDiscovery
    pub(crate) async fn builder_for(
        config_yaml: &str,
    ) -> (
        PostgresAutoDiscoveryBuilder,
        ContainerAsync<Postgres>,
        String,
    ) {
        let container = start_postgres_11_with_posgis_3_container().await;
        let connection_string = connection_string(&container).await;

        let mut config: PostgresConfig =
            serde_saphyr::from_str(config_yaml).expect("parse PostgresConfig YAML");
        config.connection_string = Some(connection_string.clone());

        let builder = PostgresAutoDiscoveryBuilder::new(
            &config,
            IdResolver::default(),
            CachePolicy::default(),
        )
        .await
        .expect("create PostgresAutoDiscoveryBuilder");
        (builder, container, connection_string)
    }

    /// Runs arbitrary setup SQL against the database behind `connection_string`.
    pub(crate) async fn seed(connection_string: &str, sql: &str) {
        let pool = PostgresPool::new(connection_string, None, None, None, 2)
            .await
            .expect("open seed pool");
        pool.get()
            .await
            .expect("acquire seed connection")
            .batch_execute(sql)
            .await
            .expect("execute seed SQL");
    }
}
