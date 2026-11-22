//! One route rendering tiles as PNG, and a Leaflet page showing them.

use std::io::Cursor;
use std::num::NonZeroU8;
use std::path::PathBuf;

use actix_web::error::ErrorInternalServerError;
use actix_web::http::header::CACHE_CONTROL;
use actix_web::{App, HttpResponse, HttpServer, web};
use anyhow::Context as _;
use clap::Parser;
use image::ImageFormat;
use martin_core::styles::render_pool::RenderPools;
use martin_core::styles::{DEFAULT_RENDERERS_PER_WORKER, DEFAULT_STATIC_RENDERERS_PER_WORKER, RenderStyle, TileSize};

/// Style rendered when `--style` is not given.
const DEFAULT_STYLE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../tests/fixtures/styles/maplibre_demo.json"
);

/// Address the server listens on.
const ADDR: (&str, u16) = ("127.0.0.1", 3000);

/// Pixel ratio of every rendered tile, so 512 px tiles stay sharp on high-DPI screens.
const RETINA: NonZeroU8 = NonZeroU8::new(2).expect("2 is non-zero");

/// Command line of the server.
#[derive(Debug, Parser)]
struct Args {
    /// Style JSON file to render.
    #[arg(long, default_value = DEFAULT_STYLE)]
    style: PathBuf,
}

/// What every request shares.
struct AppState {
    /// Worker threads rendering tiles.
    pools: RenderPools,
    /// Style every tile is rendered from.
    style: RenderStyle,
}

/// Parses the command line, spawns the render pools and serves until stopped.
pub async fn run() -> anyhow::Result<()> {
    let args = Args::parse();
    let style = args
        .style
        .canonicalize()
        .with_context(|| format!("reading style {}", args.style.display()))?;
    let pools = RenderPools::new(
        None,
        TileSize::Px512,
        DEFAULT_RENDERERS_PER_WORKER,
        DEFAULT_STATIC_RENDERERS_PER_WORKER,
        None,
    )
    .context("starting the render pools")?;
    let state = web::Data::new(AppState {
        pools,
        style: style.clone().into(),
    });

    #[expect(clippy::print_stdout, reason = "example prints startup banner")]
    {
        println!("Rendering {}", style.display());
        println!("Server running on http://{}:{}", ADDR.0, ADDR.1);
    }

    HttpServer::new(move || {
        App::new()
            .app_data(state.clone())
            .route("/", web::get().to(index))
            .route("/{z}/{x}/{y}", web::get().to(tile))
    })
    .bind(ADDR)?
    .run()
    .await?;
    Ok(())
}

/// Renders tile `z/x/y` and answers it as PNG.
async fn tile(path: web::Path<(u8, u32, u32)>, state: web::Data<AppState>) -> actix_web::Result<HttpResponse> {
    let (z, x, y) = path.into_inner();
    let image = state
        .pools
        .render_tile_with_pixel_ratio(state.style.clone(), z, x, y, RETINA)
        .await
        .map_err(ErrorInternalServerError)?;
    let mut png = Vec::new();
    image
        .write_to(&mut Cursor::new(&mut png), ImageFormat::Png)
        .map_err(ErrorInternalServerError)?;
    Ok(HttpResponse::Ok()
        .content_type("image/png")
        .insert_header((CACHE_CONTROL, "max-age=3600"))
        .body(png))
}

/// Answers the Leaflet page.
async fn index() -> HttpResponse {
    HttpResponse::Ok()
        .content_type("text/html")
        .body(include_str!("index.html"))
}
