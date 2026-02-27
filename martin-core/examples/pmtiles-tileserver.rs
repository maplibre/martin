use actix_web::{App, HttpResponse, HttpServer, Result as ActixResult, web};
use martin_core::tiles::pmtiles::{PmtCache, PmtCacheInstance, PmtilesSource};
use martin_core::tiles::{Source as _, UrlQuery};
use martin_tile_utils::TileCoord;
use serde::Deserialize;

#[derive(Deserialize)]
struct TileRequest {
    z: u8,
    x: u32,
    y: u32,
}

async fn get_tile(
    path: web::Path<TileRequest>,
    state: web::Data<PmtilesSource>,
) -> ActixResult<HttpResponse> {
    let xyz = TileCoord {
        z: path.z,
        x: path.x,
        y: path.y,
    };

    let tile_data = state
        .get_tile(xyz, Option::<&UrlQuery>::None)
        .await
        .map_err(actix_web::error::ErrorInternalServerError)?;

    if tile_data.is_empty() {
        Ok(HttpResponse::NotFound().finish())
    } else {
        Ok(HttpResponse::Ok()
            .content_type("image/webp")
            .body(tile_data))
    }
}

async fn get_style(state: web::Data<PmtilesSource>) -> HttpResponse {
    let tilejson = state.get_tilejson();

    let style = serde_json::json!({
        "version": 8,
        "name": "PMTiles Example",
        "projection": {
            "type": "globe"
        },
        "sources": {
            "pmtiles": {
                "type": "raster",
                "tiles": ["http://localhost:3000/tiles/{z}/{x}/{y}"],
                "minzoom": tilejson.minzoom.unwrap_or(0),
                "maxzoom": tilejson.maxzoom.unwrap_or(5),
                "bounds": tilejson.bounds.as_ref().map_or([-180.0, -85.0511, 180.0, 85.0511], |b| [b.left, b.bottom, b.right, b.top]),
                "tileSize": 512
            }
        },
        "layers": [
            {
                "id": "raster",
                "type": "raster",
                "source": "pmtiles"
            }
        ]
    });

    HttpResponse::Ok().json(style)
}

async fn index() -> HttpResponse {
    HttpResponse::Ok()
        .content_type("text/html")
        .body(include_str!("pmtiles-tileserver.html"))
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    env_logger::init();

    let pmtiles_path = std::path::PathBuf::from("tests/fixtures/pmtiles2/webp2.pmtiles")
        .canonicalize()
        .expect("Failed to canonicalize PMTiles path");

    let url = url::Url::from_file_path(&pmtiles_path).expect("Failed to convert path to URL");

    let (store, path) = object_store::parse_url_opts(&url, std::iter::empty::<(&str, &str)>())
        .expect("Failed to parse object store URL");

    let cache = PmtCacheInstance::new(0, PmtCache::default());
    let source = PmtilesSource::new(cache, "webp2".to_string(), store, path)
        .await
        .expect("Failed to create PMTiles source");

    let state = web::Data::new(source);

    println!("Starting server at http://localhost:3000");

    HttpServer::new(move || {
        App::new()
            .app_data(state.clone())
            .route("/", web::get().to(index))
            .route("/style.json", web::get().to(get_style))
            .route("/tiles/{z}/{x}/{y}", web::get().to(get_tile))
    })
    .bind(("127.0.0.1", 3000))?
    .run()
    .await
}
