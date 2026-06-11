#![cfg(feature = "test-pg")]

use std::collections::BTreeMap;
use std::time::Duration;

use actix_web::dev::ServiceResponse;
use actix_web::test::{TestRequest, call_service, init_service, read_body_json};
use actix_web::web::Data;
use indoc::formatdoc;
use insta::assert_yaml_snapshot;
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

/// Asserts the current (no-reloader) behavior: tables that exist at startup are published, but
/// tables created out-of-band after `resolve()` are NOT picked up without a restart.
///
/// When PR #2841 (PostgresReloader) lands, the negative assertions here should flip to positive.
#[actix_rt::test]
#[tracing_test::traced_test]
async fn pg_startup_catalog_is_static_without_reloader() {
    let (_container, connstr) = start_postgis().await;

    // Seeded before `resolve()` so it is part of the startup catalog.
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
          auto_publish:
            from_schemas: public
    "};

    let mut config = utils::mock_cfg(&yaml);
    let resolver = IdResolver::new(&[]);
    let state = config.resolve(&resolver).await.expect("resolve config");

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

    // `reload_alpha` was part of the startup baseline, so it must appear.
    let tiles = catalog_tiles(&app).await;
    assert!(
        tiles.contains_key("reload_alpha"),
        "startup-seeded table must appear in /catalog, got: {tiles:?}"
    );

    // Create a second table out-of-band, then give the server a generous window to (not) react.
    // On main there is no PG reloader, so this table must remain invisible until restart.
    seed(
        &connstr,
        "CREATE TABLE public.reload_beta (id serial PRIMARY KEY, geom geometry(Point, 4326));
         INSERT INTO public.reload_beta (geom) VALUES (ST_SetSRID(ST_MakePoint(0, 0), 4326));",
    )
    .await;
    tokio::time::sleep(Duration::from_secs(3)).await;

    let tiles = catalog_tiles(&app).await;
    let managed: BTreeMap<String, Value> = ["reload_alpha", "reload_beta"]
        .into_iter()
        .filter_map(|id| tiles.get(id).map(|v| (id.to_string(), v.clone())))
        .collect();
    assert_yaml_snapshot!(managed, @r"
    reload_alpha:
      content_type: application/x-protobuf
      description: public.reload_alpha.geom
    ");

    // Tile requests for the still-unknown source must 404, not serve a tile.
    let resp = call_service(
        &app,
        TestRequest::get().uri("/reload_beta/0/0/0").to_request(),
    )
    .await;
    assert_eq!(
        resp.status().as_u16(),
        404,
        "without a reloader, a post-startup table must not be tile-served"
    );
}
