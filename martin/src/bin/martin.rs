use std::env;

use clap::Parser as _;
use martin::MartinResult;
use martin::config::args::Args;
use martin::config::file::{Config, read_config};
use martin::config::primitives::env::OsEnv;
use martin::logging::{ensure_martin_core_log_level_matches, init_tracing};
use martin::srv::new_server;
use tracing::{error, info};

const VERSION: &str = env!("CARGO_PKG_VERSION");

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

    args.merge_into_config(&mut config, &env)?;
    config.finalize()?;
    let sources = config.resolve().await?;

    if let Some(file_name) = save_config {
        config.save_to_file(file_name.as_path())?;
    } else {
        info!("Use --save-config to save or print Martin configuration.");
    }

    #[cfg(all(feature = "webui", not(docsrs)))]
    let web_ui_mode = config.srv.web_ui.unwrap_or_default();

    let route_prefix = config.srv.route_prefix.clone();
    let (server, listen_addresses) = new_server(config.srv, sources)?;
    let base_url = if let Some(ref prefix) = route_prefix {
        format!("http://{listen_addresses}{prefix}/")
    } else {
        format!("http://{listen_addresses}/")
    };

    #[cfg(all(feature = "webui", not(docsrs)))]
    if web_ui_mode == martin::config::args::WebUiMode::EnableForAll {
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
async fn main() {
    let filter = ensure_martin_core_log_level_matches(env::var("RUST_LOG").ok(), "martin=");
    init_tracing(&filter, env::var("RUST_LOG_FORMAT").ok());

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
