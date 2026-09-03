#![cfg(feature = "test-pg")]

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use actix_web::dev::ServiceResponse;
use actix_web::test::{TestRequest, call_service, init_service, read_body_json};
use actix_web::web::Data;
use indoc::formatdoc;
use insta::assert_yaml_snapshot;
use martin::config::file::ProcessConfig;
use martin::config::file::reload::postgres::PostgresReloader;
use martin::config::file::srv::SrvConfig;
use martin::config::primitives::IdResolver;
use martin_core::tiles::postgres::{PostgresPool, RetryTimeout};
use serde_json::Value;
use testcontainers_modules::postgres::Postgres;
use testcontainers_modules::testcontainers::runners::AsyncRunner as _;
use testcontainers_modules::testcontainers::{ContainerAsync, ImageExt as _};

pub mod utils;

const RELOAD_INTERVAL: Duration = Duration::from_millis(100);
const CATALOG_TIMEOUT: Duration = Duration::from_secs(20);

/// Launches a throwaway `PostGIS` container so the run never touches the shared `just start` DB.
async fn start_postgis() -> (ContainerAsync<Postgres>, String) {
    let container = Postgres::default()
        .with_name("postgis/postgis")
        .with_tag("11-3.0") // purposely very old and stable
        .start()
        .await
        .expect("PostGIS container failed to start (is Docker running?)");
    let host = container.get_host().await.expect("container host");
    let port = container
        .get_host_port_ipv4(5432)
        .await
        .expect("container port");
    let connstr = format!("postgres://postgres:postgres@{host}:{port}/postgres?sslmode=disable");
    (container, connstr)
}

async fn seed(connstr: &str, sql: &str) {
    let pool = PostgresPool::new(connstr, None, None, None, 2, RetryTimeout::default())
        .await
        .expect("open seed pool");
    pool.get()
        .await
        .expect("acquire seed connection")
        .batch_execute(sql)
        .await
        .expect("execute seed SQL");
}

trait App:
    actix_web::dev::Service<actix_http::Request, Response = ServiceResponse, Error = actix_web::Error>
{
}
impl<T> App for T where
    T: actix_web::dev::Service<
            actix_http::Request,
            Response = ServiceResponse,
            Error = actix_web::Error,
        >
{
}

async fn catalog_tiles(app: &impl App) -> serde_json::Map<String, Value> {
    let req = TestRequest::get().uri("/catalog").to_request();
    let resp = call_service(app, req).await;
    assert!(resp.status().is_success(), "/catalog failed: {resp:?}");
    let body: Value = read_body_json(resp).await;
    body.get("tiles")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default()
}

const MANAGED: [&str; 3] = ["reload_alpha", "reload_boundary", "reload_beta"];

async fn managed_tiles(app: &impl App) -> BTreeMap<String, Value> {
    let tiles = catalog_tiles(app).await;
    MANAGED
        .into_iter()
        .filter_map(|id| tiles.get(id).map(|v| (id.to_owned(), v.clone())))
        .collect()
}

/// Polls `/catalog` until `id` is (or is not) listed, instead of sleeping a fixed time.
async fn wait_for_catalog(app: &impl App, id: &str, present: bool) {
    let deadline = Instant::now() + CATALOG_TIMEOUT;
    loop {
        let tiles = catalog_tiles(app).await;
        if tiles.contains_key(id) == present {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "{id} {} in /catalog after {CATALOG_TIMEOUT:?}: {tiles:?}",
            if present {
                "never appeared"
            } else {
                "still listed"
            }
        );
        tokio::time::sleep(RELOAD_INTERVAL).await;
    }
}

async fn tile_status(app: &impl App, id: &str) -> u16 {
    call_service(
        app,
        TestRequest::get().uri(&format!("/{id}/0/0/0")).to_request(),
    )
    .await
    .status()
    .as_u16()
}

#[actix_rt::test]
#[tracing_test::traced_test]
async fn catalog_follows_create_and_drop_through_the_reloader() {
    let (_container, connstr) = start_postgis().await;

    seed(
        &connstr,
        "CREATE TABLE public.reload_alpha (id serial PRIMARY KEY, geom geometry(Point, 4326));
         INSERT INTO public.reload_alpha (geom) VALUES (ST_SetSRID(ST_MakePoint(0, 0), 4326));
         CREATE TABLE public.reload_boundary (id serial PRIMARY KEY, geom geometry(Point, 4326));",
    )
    .await;

    let yaml = formatdoc! {"
        postgres:
          connection_string: '{connstr}'
          reload_interval: {}ms
          auto_publish:
            from_schemas: public
    ", RELOAD_INTERVAL.as_millis()};

    let mut config = utils::mock_cfg(&yaml).await;
    let resolver = IdResolver::new(&[]);
    let state = config.resolve(&resolver).await.expect("resolve config");
    let tsm = state.tile_manager.clone();

    let mut reloader = PostgresReloader::new(
        tsm.clone(),
        resolver,
        config
            .postgres
            .iter()
            .next()
            .cloned()
            .expect("one postgres config"),
        config.cache.policy(),
        &ProcessConfig::default(),
    );
    let warnings = reloader.init().await.expect("init postgres sources");
    assert!(warnings.is_empty(), "unexpected warnings: {warnings:?}");

    seed(&connstr, "DROP TABLE public.reload_boundary;").await;
    let _driver = reloader
        .start()
        .expect("reload_interval > 0 spawns the driver");

    let app = init_service(
        actix_web::App::new()
            .app_data(Data::new(
                martin::srv::Catalog::new(
                    #[cfg(any(feature = "sprites", feature = "fonts", feature = "styles"))]
                    &state,
                )
                .expect("catalog"),
            ))
            .app_data(Data::new(tsm))
            .app_data(Data::new(SrvConfig::default()))
            .configure(|c| martin::srv::router(c, &SrvConfig::default())),
    )
    .await;

    wait_for_catalog(&app, "reload_boundary", false).await;
    assert_yaml_snapshot!(managed_tiles(&app).await, @"
    reload_alpha:
      content_type: application/x-protobuf
      description: public.reload_alpha.geom
    ");
    assert_eq!(tile_status(&app, "reload_boundary").await, 404);

    seed(
        &connstr,
        "CREATE TABLE public.reload_beta (id serial PRIMARY KEY, geom geometry(Point, 4326));
         INSERT INTO public.reload_beta (geom) VALUES (ST_SetSRID(ST_MakePoint(0, 0), 4326));",
    )
    .await;
    wait_for_catalog(&app, "reload_beta", true).await;
    assert_yaml_snapshot!(managed_tiles(&app).await, @"
    reload_alpha:
      content_type: application/x-protobuf
      description: public.reload_alpha.geom
    reload_beta:
      content_type: application/x-protobuf
      description: public.reload_beta.geom
    ");
    assert_eq!(tile_status(&app, "reload_beta").await, 200);

    seed(&connstr, "DROP TABLE public.reload_beta;").await;
    wait_for_catalog(&app, "reload_beta", false).await;
    assert_yaml_snapshot!(managed_tiles(&app).await, @"
    reload_alpha:
      content_type: application/x-protobuf
      description: public.reload_alpha.geom
    ");
    assert_eq!(tile_status(&app, "reload_beta").await, 404);
    assert_eq!(tile_status(&app, "reload_alpha").await, 200);
}
