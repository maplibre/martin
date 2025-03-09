use clap::Parser;
use martin::args::{Args, OsEnv};
use martin::srv::new_server;
use martin::{read_config, Config, MartinResult};
use martin_observability_utils::{LogFormat, LogLevel, MartinObservability};
use tracing::{error, event_enabled, info};

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
        config.save_to_file(file_name)?;
    } else {
        info!("Use --save-config to save or print Martin configuration.");
    }

    #[cfg(feature = "webui")]
    let web_ui_mode = config.srv.web_ui.unwrap_or_default();

    let (server, listen_addresses) = new_server(config.srv, sources)?;
    info!("Martin has been started on {listen_addresses}.");
    info!("Use http://{listen_addresses}/catalog to get the list of available sources.");

    #[cfg(feature = "webui")]
    if web_ui_mode == martin::args::WebUiMode::EnableForAll {
        tracing::warn!("Web UI is enabled for all connections at http://{listen_addresses}/");
    } else {
        info!(
            "Web UI is disabled. Use `--webui enable-for-all` in CLI or a config value to enable it for all connections."
        );
    }

    server.await
}

#[actix_web::main]
async fn main() {
    // since logging is not yet available, we have to manually check the locations
    let log_filter = LogLevel::from_env_var("RUST_LOG")
        .or_from_argument("--log-level")
        .or_in_config_file("--config", "log_level")
        .lossy_parse_to_filter_with_default("info");
    let log_format = LogFormat::from_env_var("MARTIN_LOG_FORMAT");
    MartinObservability::from((log_filter, log_format))
        .with_initialised_log_tracing()
        .set_global_subscriber();

    if let Err(e) = start(Args::parse()).await {
        // Ensure the message is printed, even if the logging is disabled
        if event_enabled!(tracing::Level::ERROR) {
            error!("{e}");
        } else {
            eprintln!("{e}");
        }
        std::process::exit(1);
    }
}
