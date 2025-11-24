use actix_web::{App, HttpResponse, HttpServer, Result as ActixResult, web};
use martin_core::tiles::pmtiles::{PmtCache, PmtCacheInstance, PmtilesSource};
use martin_core::tiles::{Source, UrlQuery};
use martin_tile_utils::TileCoord;
use serde::Deserialize;
use std::sync::Arc;

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
        return Ok(HttpResponse::NotFound().finish());
    }

    Ok(HttpResponse::Ok()
        .content_type("image/webp")
        .body(tile_data))
}

async fn get_style(state: web::Data<PmtilesSource>) -> ActixResult<HttpResponse> {
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

    Ok(HttpResponse::Ok().json(style))
}

async fn index() -> ActixResult<HttpResponse> {
    let html = r#"<!DOCTYPE html>
<html>
<head>
    <meta charset="utf-8">
    <title>PMTiles Tile Server Example</title>
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <link rel="stylesheet" href="https://unpkg.com/maplibre-gl@5.13.0/dist/maplibre-gl.css">
    <script src="https://unpkg.com/maplibre-gl@5.13.0/dist/maplibre-gl.js"></script>
    <style>
        body { margin: 0; padding: 0; }
        #map { position: absolute; top: 0; bottom: 0; width: 100%; }
    </style>
</head>
<body>
    <div id="map"></div>
    <script>
        const map = new maplibregl.Map({
            container: 'map',
            style: 'http://localhost:3000/style.json',
            center: [0, 20],
            zoom: 2
        });

        map.addControl(new maplibregl.NavigationControl());
    </script>
</body>
</html>"#;

    Ok(HttpResponse::Ok().content_type("text/html").body(html))
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
