//! Serves raster tiles rendered from a `MapLibre` style through martin-core's render pools.
//!
//! `render-tileserver [--style <style.json>]`, then open <http://127.0.0.1:3000>.

mod server;

/// Starts the tile server.
#[actix_web::main]
async fn main() -> anyhow::Result<()> {
    server::run().await
}
