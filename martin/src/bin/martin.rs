use std::fmt::Write;

use clap::Parser;
use log::{error, info, log_enabled};
use martin::MartinResult;
use martin::config::args::Args;
use martin::config::file::{Config, read_config};
use martin::srv::new_server;
use martin_core::config::env::OsEnv;

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

    let (server, listen_addresses) = new_server(config.srv, sources)?;
    info!("Martin has been started on {listen_addresses}.");
    info!("Use http://{listen_addresses}/catalog to get the list of available sources.");

    #[cfg(all(feature = "webui", not(docsrs)))]
    if web_ui_mode == martin::config::args::WebUiMode::EnableForAll {
        log::warn!("Web UI is enabled for all connections at http://{listen_addresses}/");
    } else {
        info!(
            "Web UI is disabled. Use `--webui enable-for-all` in CLI or a config value to enable it for all connections."
        );
    }

    server.await
}

#[actix_web::main]
async fn main() {
    let mut log_filter = std::env::var("RUST_LOG").unwrap_or("martin=info".to_string());
    // if we don't have martin_core set, this can hide parts of our logs unintentionally
    if log_filter.contains("martin=")
        && !log_filter.contains("martin_core=")
        && let Some(level) = log_filter
            .split(',')
            .find_map(|s| s.strip_prefix("martin="))
    {
        let level = level.to_string();
        let _ = write!(log_filter, ",martin_core={level}");
    }
    env_logger::builder().parse_filters(&log_filter).init();

    if let Err(e) = start(Args::parse()).await {
        // Ensure the message is printed, even if the logging is disabled
        if log_enabled!(log::Level::Error) {
            error!("{e}");
        } else {
            eprintln!("{e}");
        }
        std::process::exit(1);
    }
}
