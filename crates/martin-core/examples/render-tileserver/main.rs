//! Serves raster tiles rendered from a `MapLibre` style through martin-core's render pools.
//!
//! `render-tileserver [--style <style.json>]`, then open <http://127.0.0.1:3000>.

#[cfg(target_os = "linux")]
mod server;

/// Starts the tile server.
#[cfg(target_os = "linux")]
#[actix_web::main]
async fn main() -> anyhow::Result<()> {
    server::run().await
}

/// Fails, as Martin renders only on Linux.
#[cfg(not(target_os = "linux"))]
fn main() -> anyhow::Result<()> {
    anyhow::bail!("render-tileserver needs Linux, the only platform Martin renders on")
}
