use actix_web::web::Data;
use actix_web::{HttpResponse, Responder, middleware, route};
use serde::{Deserialize, Serialize};

use crate::MartinResult;
#[cfg(feature = "_catalog")]
use crate::config::file::ServerState;

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
    pub fn new(#[cfg(feature = "_catalog")] state: &ServerState) -> MartinResult<Self> {
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

#[route(
    "/catalog",
    method = "GET",
    method = "HEAD",
    wrap = "middleware::Compress::default()"
)]
async fn get_catalog(catalog: Data<Catalog>) -> impl Responder {
    HttpResponse::Ok().json(catalog)
}

#[cfg(all(feature = "webui", not(docsrs)))]
pub mod webui {
    include!(concat!(env!("OUT_DIR"), "/generated.rs"));
}

/// Root path in case web front is disabled.
#[cfg(any(not(feature = "webui"), docsrs))]
#[route("/", method = "GET", method = "HEAD")]
async fn get_index_no_ui() -> &'static str {
    "Martin server is running. The WebUI feature was disabled at the compile time.\n\n\
    A list of all available sources is available at http://<host>/catalog\n\n\
    See documentation https://github.com/maplibre/martin"
}

/// Root path in case web front is disabled and the `webui` feature is enabled.
#[cfg(all(feature = "webui", not(docsrs)))]
#[route("/", method = "GET", method = "HEAD")]
async fn get_index_ui_disabled() -> &'static str {
    "Martin server is running.\n\n
    The WebUI feature can be enabled with the --webui enable-for-all CLI flag or in the config file, making it available to all users.\n\n
    A list of all available sources is available at http://<host>/catalog\n\n\
    See documentation https://github.com/maplibre/martin"
}
