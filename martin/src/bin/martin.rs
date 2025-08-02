use clap::Parser;
use log::{error, info, log_enabled, warn};
use martin::args::{Args, OsEnv};
use martin::srv::new_server;
use martin::{Config, LogFormatOptions, MartinResult, ReloadableTracingConfiguration, read_config};

const VERSION: &str = env!("CARGO_PKG_VERSION");

async fn start(args: Args) -> MartinResult<()> {
    let trace = ReloadableTracingConfiguration::init_global_registry("martin=info");
    if let Ok(fmt) = std::env::var("MARTIN_LOG_FORMAT") {
        use clap::ValueEnum;
        if let Ok(fmt) = LogFormatOptions::from_str(&fmt, true) {
            trace.reload_fmt(fmt);
        } else {
            warn!("ignoring invalid log format for MARTIN_LOG_FORMAT");
        }
    }
    if let Some(fmt) = args.meta.log_format {
        trace.reload_fmt(fmt);
    }
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

    if let Some(observability) = &config.srv.observability {
        if let Some(fmt) = observability.log_format {
            trace.reload_fmt(fmt);
        }
    }
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
