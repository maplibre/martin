use std::future::Future;
use std::pin::Pin;
use std::string::ToString as _;
use std::time::Duration;

use actix_web::http::header::CACHE_CONTROL;
use actix_web::middleware::{NormalizePath, TrailingSlash};
use actix_web::web::Data;
use actix_web::{App, HttpResponse, HttpServer, Responder, middleware, route, web};
use futures::TryFutureExt as _;
#[cfg(feature = "lambda")]
use lambda_web::{is_running_on_lambda, run_actix_on_lambda};
use tracing_actix_web::TracingLogger;

#[cfg(all(feature = "webui", not(docsrs)))]
use crate::config::args::WebUiMode;
#[cfg(feature = "_catalog")]
use crate::config::file::ServerState;
use crate::config::file::srv::{KEEP_ALIVE_DEFAULT, LISTEN_ADDRESSES_DEFAULT, SrvConfig};
use crate::srv::admin::Catalog;
use crate::{MartinError, MartinResult};

/// List of keywords that cannot be used as source IDs. Some of these are reserved for future use.
/// Reserved keywords must never end in a "dot number" (e.g. ".1").
/// This list is documented in the `docs/src/using.md` file, which should be kept in sync.
pub const RESERVED_KEYWORDS: &[&str] = &[
    "_", "catalog", "config", "font", "health", "help", "index", "manifest", "metrics", "refresh",
    "reload", "sprite", "status",
];

#[cfg(any(feature = "_tiles", feature = "fonts", feature = "sprites"))]
pub fn map_internal_error<T: std::fmt::Display>(e: T) -> actix_web::Error {
    tracing::error!("{e}");
    actix_web::error::ErrorInternalServerError(e.to_string())
}

/// Helper struct for debounced warning messages in redirect handlers.
/// Ensures warnings are logged no more than once per hour to avoid log spam.
#[cfg(feature = "_catalog")]
pub struct DebouncedWarning {
    last_warning: std::sync::LazyLock<tokio::sync::Mutex<std::time::Instant>>,
}

#[cfg(feature = "_catalog")]
impl DebouncedWarning {
    /// Create a new `DebouncedWarning` instance
    pub const fn new() -> Self {
        Self {
            last_warning: std::sync::LazyLock::new(|| {
                tokio::sync::Mutex::new(std::time::Instant::now())
            }),
        }
    }

    /// Execute the provided closure at most once per hour.
    /// This allows tracing's log filtering to work correctly by keeping the warn! call site
    /// in the caller's context.
    pub async fn once_per_hour<F: FnOnce()>(&self, f: F) {
        let mut last = self.last_warning.lock().await;
        if last.elapsed() >= Duration::from_secs(3600) {
            *last = std::time::Instant::now();
            f();
        }
    }
}

/// Return 200 OK if healthy. Used for readiness and liveness probes.
#[route("/health", method = "GET", method = "HEAD")]
async fn get_health() -> impl Responder {
    HttpResponse::Ok()
        .insert_header((CACHE_CONTROL, "no-cache"))
        .message_body("OK")
}

pub fn router(cfg: &mut web::ServiceConfig, usr_cfg: &SrvConfig) {
    // If route_prefix is configured, wrap all routes in a scope
    if let Some(prefix) = &usr_cfg.route_prefix {
        cfg.service(web::scope(prefix).configure(|cfg| {
            register_services(
                cfg,
                #[cfg(all(feature = "webui", not(docsrs)))]
                usr_cfg,
            );
        }));
    } else {
        register_services(
            cfg,
            #[cfg(all(feature = "webui", not(docsrs)))]
            usr_cfg,
        );
    }
}

/// Helper function to register all services
fn register_services(
    cfg: &mut web::ServiceConfig,
    #[cfg(all(feature = "webui", not(docsrs)))] usr_cfg: &SrvConfig,
) {
    cfg.service(get_health)
        .service(crate::srv::admin::get_catalog);

    #[cfg(feature = "_tiles")]
    {
        // Register tile format suffix redirects BEFORE the main tile route
        // because Actix-Web matches routes in registration order
        cfg.service(crate::srv::tiles::content::redirect_tile_ext)
            .service(crate::srv::tiles::metadata::get_source_info)
            .service(crate::srv::tiles::content::get_tile);

        // Register /tiles/ prefix redirect after main tile route
        cfg.service(crate::srv::tiles::content::redirect_tiles);
    }

    #[cfg(feature = "sprites")]
    cfg.service(crate::srv::sprites::get_sprite_sdf_json)
        .service(crate::srv::sprites::redirect_sdf_sprites_json)
        .service(crate::srv::sprites::get_sprite_json)
        .service(crate::srv::sprites::redirect_sprites_json)
        .service(crate::srv::sprites::get_sprite_sdf_png)
        .service(crate::srv::sprites::redirect_sdf_sprites_png)
        .service(crate::srv::sprites::get_sprite_png)
        .service(crate::srv::sprites::redirect_sprites_png);

    #[cfg(feature = "fonts")]
    cfg.service(crate::srv::fonts::get_font)
        .service(crate::srv::fonts::redirect_fonts);

    #[cfg(feature = "styles")]
    cfg.service(crate::srv::styles::get_style_json)
        .service(crate::srv::styles::redirect_styles);

    #[cfg(all(feature = "unstable-rendering", target_os = "linux"))]
    cfg.service(crate::srv::styles_rendering::get_style_rendered);

    #[cfg(all(feature = "webui", not(docsrs)))]
    {
        // TODO: this can probably be simplified with a wrapping middleware,
        //       which would share usr_cfg from Data<> with all routes.
        if usr_cfg.web_ui.unwrap_or_default() == WebUiMode::EnableForAll {
            cfg.service(actix_web_static_files::ResourceFiles::new(
                "/",
                crate::srv::admin::webui::generate(),
            ));
        } else {
            cfg.service(crate::srv::admin::get_index_ui_disabled);
        }
    }

    #[cfg(any(not(feature = "webui"), docsrs))]
    cfg.service(crate::srv::admin::get_index_no_ui);
}

type Server = Pin<Box<dyn Future<Output = MartinResult<()>>>>;

/// Create a future for an Actix web server together with the listening address.
pub fn new_server(
    config: SrvConfig,
    #[cfg(feature = "_catalog")]
    state: ServerState,
) -> MartinResult<(Server, String)> {
    #[cfg(feature = "metrics")]
    let prometheus = {
        let metrics_endpoint = if let Some(prefix) = &config.route_prefix {
            format!("{prefix}/_/metrics")
        } else {
            "/_/metrics".to_string()
        };
        actix_web_prom::PrometheusMetricsBuilder::new("martin")
            .endpoint(&metrics_endpoint)
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
            .map_err(|err| MartinError::MetricsIntialisationError(err))?
    };
    let catalog = Catalog::new(
      #[cfg(feature = "_catalog")]
        &state,
    )?;

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
            .app_data(Data::new(state.tile_cache.clone()));

        #[cfg(feature = "sprites")]
        let app = app
            .app_data(Data::new(state.sprites.clone()))
            .app_data(Data::new(state.sprite_cache.clone()));

        #[cfg(feature = "fonts")]
        let app = app
            .app_data(Data::new(state.fonts.clone()))
            .app_data(Data::new(state.font_cache.clone()));

        #[cfg(feature = "styles")]
        let app = app.app_data(Data::new(state.styles.clone()));

        let app = app.wrap(middleware::Condition::new(
            cors_middleware.is_some(),
            cors_middleware.unwrap_or_default(),
        ));

        #[cfg(feature = "metrics")]
        let app = app.wrap(prometheus.clone());

        app.wrap(TracingLogger::default())
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
