use clap::Parser;
use martin::args::{Args, OsEnv};
use martin::srv::new_server;
use martin::{read_config, Config, MartinResult};
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
    setup_logging();

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

fn setup_logging() {
    use tracing_subscriber::filter::EnvFilter;
    use tracing_subscriber::fmt::Layer;
    use tracing_subscriber::prelude::*;
    // transform log records into `tracing` `Event`s.
    tracing_log::LogTracer::builder()
        .with_interest_cache(tracing_log::InterestCacheConfig::default())
        .init()
        .expect("the global logger to only be set once");

    let log_format = LogFormat::from_env();
    let registry = tracing_subscriber::registry()
        .with(
            EnvFilter::builder()
                .with_default_directive(tracing::Level::INFO.into())
                .from_env_lossy(),
        )
        .with((log_format == LogFormat::Compact).then(|| Layer::default().compact()))
        .with((log_format == LogFormat::Pretty).then(|| Layer::default().pretty()))
        .with((log_format == LogFormat::Json).then(|| Layer::default().json()));
    tracing::subscriber::set_global_default(registry)
        .expect("since martin has not set the global_default, no global default is set");
}
#[derive(PartialEq, Eq)]
enum LogFormat {
    Json,
    Pretty,
    Compact,
}
impl LogFormat {
    fn from_env() -> Self {
        match std::env::var("MARTIN_LOG_FORMAT")
            .unwrap_or_default()
            .as_str()
        {
            "pretty" | "verbose" => LogFormat::Pretty,
            "json" | "jsonl" => LogFormat::Json,
            _ => LogFormat::Compact,
        }
    }
}
