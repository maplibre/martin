#![cfg(feature = "test-pg")]

//! End-to-end test for the `PostgreSQL` hot-reloader: drives the production path
//! (`PostgresReloader::start()` on a live `PollTrigger`) against a throwaway `PostGIS`
//! container and asserts through the public HTTP surface (`/catalog` and a tile fetch).

use std::collections::BTreeMap;
use std::time::Duration;

use actix_web::dev::ServiceResponse;
use actix_web::test::{TestRequest, call_service, init_service, read_body, read_body_json};
use actix_web::web::Data;
use indoc::formatdoc;
use insta::assert_yaml_snapshot;
use martin::config::file::ProcessConfig;
use martin::config::file::postgres::PostgresConfig;
use martin::config::file::reload::postgres::PostgresReloader;
use martin::config::file::srv::SrvConfig;
use martin::config::primitives::IdResolver;
use martin_core::tiles::postgres::PostgresPool;
use serde_json::Value;
use testcontainers_modules::postgres::Postgres;
use testcontainers_modules::testcontainers::runners::AsyncRunner as _;
use testcontainers_modules::testcontainers::{ContainerAsync, ImageExt as _};

pub mod utils;

/// Launches a throwaway `PostGIS` container so the run never touches the shared `just start` DB.
async fn start_postgis() -> (ContainerAsync<Postgres>, String) {
    let container = Postgres::default()
        .with_name("postgis/postgis")
        .with_tag("11-3.0") // purposely very old and stable
        .start()
        .await
        .expect("PostGIS container failed to start (is Docker running?)");
    let host = container.get_host().await.unwrap();
    let port = container.get_host_port_ipv4(5432).await.unwrap();
    let connstr = format!("postgres://postgres:postgres@{host}:{port}/postgres?sslmode=disable");
    (container, connstr)
}

async fn seed(connstr: &str, sql: &str) {
    let pool = PostgresPool::new(connstr, None, None, None, 2)
        .await
        .expect("open seed pool");
    pool.get()
        .await
        .expect("acquire seed connection")
        .batch_execute(sql)
        .await
        .expect("execute seed SQL");
}

async fn catalog_tiles(
    app: &impl actix_web::dev::Service<
        actix_http::Request,
        Response = ServiceResponse,
        Error = actix_web::Error,
    >,
) -> serde_json::Map<String, Value> {
    let req = TestRequest::get().uri("/catalog").to_request();
    let resp = call_service(app, req).await;
    assert!(resp.status().is_success(), "/catalog failed: {resp:?}");
    let body: Value = read_body_json(resp).await;
    body.get("tiles")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default()
}

/// Polls `/catalog` until the predicate returns true or the deadline is reached.
/// Used to await reloader propagation between mutations and assertions.
async fn wait_for_catalog<F>(
    app: &impl actix_web::dev::Service<
        actix_http::Request,
        Response = ServiceResponse,
        Error = actix_web::Error,
    >,
    deadline: Duration,
    label: &str,
    predicate: F,
) where
    F: Fn(&serde_json::Map<String, Value>) -> bool,
{
    let start = std::time::Instant::now();
    loop {
        let tiles = catalog_tiles(app).await;
        if predicate(&tiles) {
            return;
        }
        assert!(
            start.elapsed() <= deadline,
            "timed out waiting for catalog condition '{label}'; current tiles={tiles:?}"
        );
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

/// A table created out-of-band must surface in `/catalog` via polling, serve a real tile,
/// and disappear again once dropped — all without restarting the server.
#[actix_rt::test]
#[tracing_test::traced_test]
async fn pg_reload_publishes_and_drops_via_public_api() {
    let (_container, connstr) = start_postgis().await;

    // Seeded before `resolve()`, so it is part of the startup baseline the reloader diffs against.
    seed(
        &connstr,
        "CREATE TABLE public.reload_alpha (id serial PRIMARY KEY, geom geometry(Point, 4326));
         INSERT INTO public.reload_alpha (geom) VALUES (ST_SetSRID(ST_MakePoint(0, 0), 4326));",
    )
    .await;

    // Discovery is scoped to `public` so the container's spatial system schemas never leak in.
    let yaml = formatdoc! {"
        postgres:
          connection_string: '{connstr}'
          reload_interval: 1s
          auto_publish:
            from_schemas: public
    "};

    let mut config = utils::mock_cfg(&yaml);
    let pg_configs: Vec<PostgresConfig> = config.postgres.clone().into_iter().collect();
    let default_cache = config.cache.policy();
    let resolver = IdResolver::new(&[]);
    let state = config.resolve(&resolver).await.expect("resolve config");

    for pg_config in pg_configs {
        PostgresReloader::new(
            state.tile_manager.clone(),
            resolver.clone(),
            pg_config,
            default_cache,
            &ProcessConfig::default(),
        )
        .start();
    }

    let app = init_service(
        actix_web::App::new()
            .app_data(Data::new(
                martin::srv::Catalog::new(
                    #[cfg(any(feature = "sprites", feature = "fonts", feature = "styles"))]
                    &state,
                )
                .unwrap(),
            ))
            .app_data(Data::new(state.tile_manager.clone()))
            .app_data(Data::new(SrvConfig::default()))
            .configure(|c| martin::srv::router(c, &SrvConfig::default())),
    )
    .await;

    wait_for_catalog(&app, Duration::from_secs(15), "alpha discovered", |t| {
        t.contains_key("reload_alpha")
    })
    .await;

    // The reloader seeds its baseline from a startup discovery that runs asynchronously and does
    // not touch the catalog, so it is not observable through `/catalog`. A table created before
    // that seed completes would be folded into the baseline and never published. Sleeping past a
    // few poll intervals guarantees the seed has captured the `{reload_alpha}` baseline before we
    // mutate, so the next `reload_beta` is a genuine post-baseline addition.
    tokio::time::sleep(Duration::from_secs(3)).await;

    // A second table created out-of-band must be picked up by polling, without a server restart.
    seed(
        &connstr,
        "CREATE TABLE public.reload_beta (id serial PRIMARY KEY, geom geometry(Point, 4326));
         INSERT INTO public.reload_beta (geom) VALUES (ST_SetSRID(ST_MakePoint(0, 0), 4326));",
    )
    .await;
    wait_for_catalog(&app, Duration::from_secs(15), "beta discovered", |t| {
        t.contains_key("reload_alpha") && t.contains_key("reload_beta")
    })
    .await;

    // Filter to known ids so the snapshot is deterministic regardless of system tables.
    let tiles = catalog_tiles(&app).await;
    let managed: BTreeMap<String, Value> = ["reload_alpha", "reload_beta"]
        .into_iter()
        .filter_map(|id| tiles.get(id).map(|v| (id.to_string(), v.clone())))
        .collect();
    assert_yaml_snapshot!(managed, @r"
    reload_alpha:
      content_type: application/x-protobuf
      description: public.reload_alpha.geom
    reload_beta:
      content_type: application/x-protobuf
      description: public.reload_beta.geom
    ");

    // The reloader-instantiated `reload_beta` source serves a real tile, not just a catalog
    // entry; its single point at (0,0) lands inside tile 0/0/0.
    let tile_resp = call_service(
        &app,
        TestRequest::get().uri("/reload_beta/0/0/0").to_request(),
    )
    .await;
    let status = tile_resp.status().as_u16();
    let body = read_body(tile_resp).await;
    assert_yaml_snapshot!(
        serde_json::json!({
            "status": status,
            "body_non_empty": !body.is_empty(),
        }),
        @r"
    body_non_empty: true
    status: 200
    "
    );

    seed(&connstr, "DROP TABLE public.reload_beta;").await;
    wait_for_catalog(&app, Duration::from_secs(15), "beta removed", |t| {
        !t.contains_key("reload_beta") && t.contains_key("reload_alpha")
    })
    .await;
}
