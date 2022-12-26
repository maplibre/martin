use std::ffi::OsStr;
use std::fmt::Display;
use std::fs::File;
use std::io::Write;

use actix_web::dev::Server;
use clap::Parser;
use log::{error, info, log_enabled};
use martin::args::{Args, OsEnv};
use martin::pg::PgConfig;
use martin::srv::{new_server, RESERVED_KEYWORDS};
use martin::Error::ConfigWriteError;
use martin::{read_config, Config, IdResolver, Result};

const VERSION: &str = env!("CARGO_PKG_VERSION");

async fn start(args: Args) -> Result<Server> {
    info!("Starting Martin v{VERSION}");

    let save_config = args.meta.save_config.clone();
    let mut config = if let Some(ref cfg_filename) = args.meta.config {
        info!("Using {}", cfg_filename.display());
        read_config(cfg_filename)?
    } else {
        info!("Config file is not specified, auto-detecting sources");
        Config::default()
    };

    args.merge_into_config(&mut config, &OsEnv::default())?;
    config.finalize()?;
    let sources = config.resolve(IdResolver::new(RESERVED_KEYWORDS)).await?;

    if let Some(file_name) = save_config {
        let yaml = serde_yaml::to_string(&config).expect("Unable to serialize config");
        if file_name.as_os_str() == OsStr::new("-") {
            info!("Current system configuration:");
            println!("\n\n{yaml}\n");
        } else {
            info!(
                "Saving config to {}, use --config to load it",
                file_name.display()
            );
            File::create(file_name.clone())
                .map_err(|e| ConfigWriteError(e, file_name.clone()))?
                .write_all(yaml.as_bytes())
                .map_err(|e| ConfigWriteError(e, file_name.clone()))?;
        }
    } else if config
        .postgres
        .iter()
        .any(|v| v.as_slice().iter().any(PgConfig::is_autodetect))
    {
        info!("Martin has been configured with automatic settings.");
        info!("Use --save-config to save or print Martin configuration.");
    }

    let (server, listen_addresses) = new_server(config.srv, sources)?;
    info!("Martin has been started on {listen_addresses}.");
    info!("Use http://{listen_addresses}/catalog to get the list of available sources.");

    Ok(server)
}

#[actix_web::main]
async fn main() {
    let env = env_logger::Env::default().filter_or(env_logger::DEFAULT_FILTER_ENV, "martin=info");
    env_logger::Builder::from_env(env).init();

    start(Args::parse())
        .await
        .map_or_else(|e| on_error(e), |server| async { server.await })
        .await
        .unwrap_or_else(|e| on_error(e));
}

fn on_error<E: Display>(e: E) -> ! {
    // Ensure the message is printed, even if the logging is disabled
    if log_enabled!(log::Level::Error) {
        error!("{e}");
    } else {
        eprintln!("{e}");
    }
    std::process::exit(1);
}
