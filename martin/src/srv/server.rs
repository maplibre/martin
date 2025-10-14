use std::future::Future;
use std::pin::Pin;
use std::string::ToString;
use std::time::Duration;

use actix_web::http::header::CACHE_CONTROL;
use actix_web::middleware::{Logger, NormalizePath, TrailingSlash};
use actix_web::web::Data;
use actix_web::{App, HttpResponse, HttpServer, Responder, middleware, route, web};
use futures::TryFutureExt;
#[cfg(feature = "lambda")]
use lambda_web::{is_running_on_lambda, run_actix_on_lambda};
use serde::{Deserialize, Serialize};

#[cfg(all(feature = "webui", not(docsrs)))]
use crate::config::args::WebUiMode;
use crate::config::file::ServerState;
use crate::config::file::srv::{KEEP_ALIVE_DEFAULT, LISTEN_ADDRESSES_DEFAULT, SrvConfig};
use crate::{MartinError, MartinResult};

#[cfg(all(feature = "webui", not(docsrs)))]
mod webui {
    #![allow(clippy::unreadable_literal)]
    #![allow(clippy::too_many_lines)]
    #![allow(clippy::wildcard_imports)]
    include!(concat!(env!("OUT_DIR"), "/generated.rs"));
}

/// List of keywords that cannot be used as source IDs. Some of these are reserved for future use.
/// Reserved keywords must never end in a "dot number" (e.g. ".1").
/// This list is documented in the `docs/src/using.md` file, which should be kept in sync.
pub const RESERVED_KEYWORDS: &[&str] = &[
    "_", "catalog", "config", "font", "health", "help", "index", "manifest", "metrics", "refresh",
    "reload", "sprite", "status",
];

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Catalog {
    #[cfg(feature = "_tiles")]
    pub tiles: martin_core::tiles::catalog::TileCatalog,
    #[cfg(feature = "sprites")]
    pub sprites: martin_core::sprites::SpriteCatalog,
    #[cfg(feature = "fonts")]
    pub fonts: martin_core::fonts::FontCatalog,
    #[cfg(feature = "styles")]
    pub styles: martin_core::styles::StyleCatalog,
}

impl Catalog {
    pub fn new(#[allow(unused_variables)] state: &ServerState) -> MartinResult<Self> {
        Ok(Self {
            #[cfg(feature = "_tiles")]
            tiles: state.tiles.get_catalog(),
            #[cfg(feature = "sprites")]
            sprites: state.sprites.get_catalog()?,
            #[cfg(feature = "fonts")]
            fonts: state.fonts.get_catalog(),
            #[cfg(feature = "styles")]
            styles: state.styles.get_catalog(),
        })
    }
}

#[cfg(any(feature = "_tiles", feature = "fonts", feature = "sprites"))]
pub fn map_internal_error<T: std::fmt::Display>(e: T) -> actix_web::Error {
    log::error!("{e}");
    actix_web::error::ErrorInternalServerError(e.to_string())
}

/// Root path in case web front is disabled.
#[cfg(any(not(feature = "webui"), docsrs))]
#[route("/", method = "GET", method = "HEAD")]
#[allow(clippy::unused_async)]
async fn get_index_no_ui() -> &'static str {
    "Martin server is running. The WebUI feature was disabled at the compile time.\n\n\
    A list of all available sources is available at http://<host>/catalog\n\n\
    See documentation https://github.com/maplibre/martin"
}

/// Root path in case web front is disabled and the `webui` feature is enabled.
#[cfg(all(feature = "webui", not(docsrs)))]
#[route("/", method = "GET", method = "HEAD")]
#[allow(clippy::unused_async)]
async fn get_index_ui_disabled() -> &'static str {
    "Martin server is running.\n\n
    The WebUI feature can be enabled with the --webui enable-for-all CLI flag or in the config file, making it available to all users.\n\n
    A list of all available sources is available at http://<host>/catalog\n\n\
    See documentation https://github.com/maplibre/martin"
}

/// Return 200 OK if healthy. Used for readiness and liveness probes.
#[route("/health", method = "GET", method = "HEAD")]
#[allow(clippy::unused_async)]
async fn get_health() -> impl Responder {
    HttpResponse::Ok()
        .insert_header((CACHE_CONTROL, "no-cache"))
        .message_body("OK")
}

#[route(
    "/catalog",
    method = "GET",
    method = "HEAD",
    wrap = "middleware::Compress::default()"
)]
#[allow(clippy::unused_async)]
async fn get_catalog(catalog: Data<Catalog>) -> impl Responder {
    HttpResponse::Ok().json(catalog)
}

pub fn router(cfg: &mut web::ServiceConfig, #[allow(unused_variables)] usr_cfg: &SrvConfig) {
    cfg.service(get_health).service(get_catalog);

    #[cfg(feature = "_tiles")]
    cfg.service(crate::srv::tiles_info::get_source_info)
        .service(crate::srv::tiles::get_tile);

    #[cfg(feature = "sprites")]
    cfg.service(crate::srv::sprites::get_sprite_sdf_json)
        .service(crate::srv::sprites::get_sprite_json)
        .service(crate::srv::sprites::get_sprite_sdf_png)
        .service(crate::srv::sprites::get_sprite_png);

    #[cfg(feature = "fonts")]
    cfg.service(crate::srv::fonts::get_font);

    #[cfg(feature = "styles")]
    cfg.service(crate::srv::styles::get_style_json);

    #[cfg(all(feature = "webui", not(docsrs)))]
    {
        // TODO: this can probably be simplified with a wrapping middleware,
        //       which would share usr_cfg from Data<> with all routes.
        if usr_cfg.web_ui.unwrap_or_default() == WebUiMode::EnableForAll {
            cfg.service(actix_web_static_files::ResourceFiles::new(
                "/",
                webui::generate(),
            ));
        } else {
            cfg.service(get_index_ui_disabled);
        }
    }

    #[cfg(any(not(feature = "webui"), docsrs))]
    cfg.service(get_index_no_ui);
}

type Server = Pin<Box<dyn Future<Output = MartinResult<()>>>>;

/// Create a future for an Actix web server together with the listening address.
pub fn new_server(config: SrvConfig, state: ServerState) -> MartinResult<(Server, String)> {
    #[cfg(feature = "metrics")]
    let prometheus = actix_web_prom::PrometheusMetricsBuilder::new("martin")
        .endpoint("/_/metrics")
        // `endpoint="UNKNOWN"` instead of `endpoint="/foo/bar"`
        .mask_unmatched_patterns("UNKNOWN")
        .const_labels(
            config
                .observability
                .clone()
                .unwrap_or_default()
                .metrics
                .unwrap_or_default()
                .add_labels,
        )
        .build()
        .map_err(|err| MartinError::MetricsIntialisationError(err))?;
    let catalog = Catalog::new(&state)?;

    let keep_alive = Duration::from_secs(config.keep_alive.unwrap_or(KEEP_ALIVE_DEFAULT));
    let worker_processes = config.worker_processes.unwrap_or_else(num_cpus::get);
    let listen_addresses = config
        .listen_addresses
        .clone()
        .unwrap_or_else(|| LISTEN_ADDRESSES_DEFAULT.to_string());

    let cors_config = config.cors.clone().unwrap_or_default();
    cors_config.validate()?;
    cors_config.log_current_configuration();

    let factory = move || {
        let cors_middleware = cors_config.make_cors_middleware();

        let app = App::new()
            .app_data(Data::new(catalog.clone()))
            .app_data(Data::new(config.clone()));

        #[cfg(feature = "_tiles")]
        let app = app
            .app_data(Data::new(state.tiles.clone()))
            .app_data(Data::new(state.cache.clone()));

        #[cfg(feature = "sprites")]
        let app = app.app_data(Data::new(state.sprites.clone()));

        #[cfg(feature = "fonts")]
        let app = app.app_data(Data::new(state.fonts.clone()));

        #[cfg(feature = "styles")]
        let app = app.app_data(Data::new(state.styles.clone()));

        let app = app.wrap(middleware::Condition::new(
            cors_middleware.is_some(),
            cors_middleware.unwrap_or_default(),
        ));

        #[cfg(feature = "metrics")]
        let app = app.wrap(prometheus.clone());

        app.wrap(Logger::default())
            .wrap(NormalizePath::new(TrailingSlash::MergeOnly))
            .configure(|c| router(c, &config))
    };

    #[cfg(feature = "lambda")]
    if is_running_on_lambda() {
        let server = run_actix_on_lambda(factory).map_err(MartinError::LambdaError);
        return Ok((Box::pin(server), "(aws lambda)".into()));
    }

    let server = HttpServer::new(factory)
        .bind(listen_addresses.clone())
        .map_err(|e| MartinError::BindingError(e, listen_addresses.clone()))?
        .keep_alive(keep_alive)
        .shutdown_timeout(0)
        .workers(worker_processes)
        .run()
        .err_into();

    Ok((Box::pin(server), listen_addresses))
}

#[cfg(all(test, feature = "_tiles"))]
pub mod tests {
    use async_trait::async_trait;
    use martin_core::tiles::{BoxedSource, MartinCoreResult, Source, UrlQuery};
    use martin_tile_utils::{Encoding, Format, TileCoord, TileData, TileInfo};
    use tilejson::TileJSON;

    #[derive(Debug, Clone)]
    pub struct TestSource {
        pub id: &'static str,
        pub tj: TileJSON,
        pub data: TileData,
    }

    #[async_trait]
    impl Source for TestSource {
        fn get_id(&self) -> &str {
            self.id
        }

        fn get_tilejson(&self) -> &TileJSON {
            &self.tj
        }

        fn get_tile_info(&self) -> TileInfo {
            TileInfo::new(Format::Mvt, Encoding::Uncompressed)
        }

        fn clone_source(&self) -> BoxedSource {
            Box::new(self.clone())
        }

        async fn get_tile(
            &self,
            _xyz: TileCoord,
            _url_query: Option<&UrlQuery>,
        ) -> MartinCoreResult<TileData> {
            Ok(self.data.clone())
        }
    }
}
