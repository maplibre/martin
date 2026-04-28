use std::env;

use clap::Parser as _;
use martin::MartinResult;
use martin::config::args::Args;
#[cfg(all(feature = "webui", not(docsrs)))]
use martin::config::args::WebUiMode;
#[cfg(feature = "unstable-cog")]
use martin::config::file::reload::cog::COGReloader;
#[cfg(feature = "mbtiles")]
use martin::config::file::reload::mbtiles::MBTilesReloader;
#[cfg(feature = "pmtiles")]
use martin::config::file::reload::pmtiles::PMTilesReloader;
use martin::config::file::{Config, read_config};
#[cfg(feature = "_tiles")]
use martin::config::primitives::IdResolver;
use martin::config::primitives::env::OsEnv;
use martin::logging::{ensure_martin_core_log_level_matches, init_tracing};
#[cfg(feature = "_tiles")]
use martin::srv::RESERVED_KEYWORDS;
use martin::srv::new_server;
use tracing::{error, info};

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[hotpath::measure]
async fn start(args: Args) -> MartinResult<()> {
    info!("Starting Martin v{VERSION}");

    let env = OsEnv::default();
    let save_config = args.meta.save_config.clone();
    let mut config = if let Some(ref cfg_filename) = args.meta.config {
        info!("Using {}", cfg_filename.display());
        read_config(cfg_filename, &env)?
    } else {
        info!("Config file is not specified, auto-detecting sources");
        Config::default()
    };

    args.merge_into_config(
        &mut config,
        #[cfg(feature = "postgres")]
        &env,
    )?;
    config.finalize()?;

    #[cfg(feature = "_tiles")]
    let resolver = IdResolver::new(RESERVED_KEYWORDS);

    #[cfg(feature = "_catalog")]
    let sources = config
        .resolve(
            #[cfg(feature = "_tiles")]
            &resolver,
        )
        .await?;
    #[cfg(any(feature = "mbtiles", feature = "unstable-cog", feature = "pmtiles"))]
    let mgr = sources.tile_manager.clone();

    #[cfg(feature = "mbtiles")]
    {
        let reloader = MBTilesReloader::new(mgr.clone(), resolver.clone(), &config.mbtiles);
        if let Err(e) = reloader.start() {
            tracing::warn!("failed to start MBTilesReloader {e:?}");
        }
    }
    #[cfg(feature = "unstable-cog")]
    {
        let reloader = COGReloader::new(mgr.clone(), resolver.clone(), &config.cog);
        if let Err(e) = reloader.start() {
            tracing::warn!("failed to start COGReloader {e:?}");
        }
    }
    #[cfg(feature = "pmtiles")]
    {
        let reloader = PMTilesReloader::new(mgr.clone(), resolver.clone(), &config.pmtiles);
        if let Err(e) = reloader.start() {
            tracing::warn!("failed to start PMTilesReloader {e:?}");
        }
    }

    if let Some(file_name) = save_config {
        config.save_to_file(file_name.as_path())?;
    } else {
        info!("Use --save-config to save or print Martin configuration.");
    }

    #[cfg(all(feature = "webui", not(docsrs)))]
    let web_ui_mode = config.srv.web_ui.unwrap_or_default();

    let route_prefix = config.srv.route_prefix.clone();
    let (server, listen_addresses) = new_server(
        config.srv,
        #[cfg(feature = "_catalog")]
        sources,
    )?;
    let base_url = if let Some(ref prefix) = route_prefix {
        format!("http://{listen_addresses}{prefix}/")
    } else {
        format!("http://{listen_addresses}/")
    };

    #[cfg(all(feature = "webui", not(docsrs)))]
    if web_ui_mode == WebUiMode::EnableForAll {
        tracing::info!("Martin server is now active at {base_url}");
    } else {
        info!(
            "Web UI is disabled. Use `--webui enable-for-all` in CLI or a config value to enable it for all connections."
        );
    }
    #[cfg(not(all(feature = "webui", not(docsrs))))]
    info!("Martin server is now active. See {base_url}catalog to see available services");

    server.await
}

#[tokio::main]
#[hotpath::main]
async fn main() {
    let filter = ensure_martin_core_log_level_matches(env::var("RUST_LOG").ok(), "martin=");
    init_tracing(&filter, env::var("RUST_LOG_FORMAT").ok(), false);

    let args = Args::parse();
    if let Err(e) = start(args).await {
        // Ensure the message is printed, even if the logging is disabled
        if tracing::event_enabled!(tracing::Level::ERROR) {
            error!("{e}");
        } else {
            eprintln!("{e}");
        }
        std::process::exit(1);
    }
}
