use actix_web::dev::Server;
use clap::Parser;
use log::{error, info, warn};
use martin::config::{read_config, ConfigBuilder};
use martin::pg::config::{parse_pg_args, PgArgs, PgConfig};
use martin::source::IdResolver;
use martin::srv::config::{SrvArgs, SrvConfig};
use martin::srv::server;
use martin::srv::server::RESERVED_KEYWORDS;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::{env, io};

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Parser, Debug)]
#[command(about, version)]
pub struct Args {
    /// Database connection string
    pub connection: Vec<String>,
    /// Path to config file.
    #[arg(short, long)]
    pub config: Option<PathBuf>,
    /// Save resulting config to a file or use "-" to print to stdout.
    /// By default, only print if sources are auto-detected.
    #[arg(long)]
    pub save_config: Option<PathBuf>,
    /// [Deprecated] Scan for new sources on sources list requests
    #[arg(short, long, hide = true)]
    pub watch: bool,
    #[command(flatten)]
    srv: SrvArgs,
    #[command(flatten)]
    pg: PgArgs,
}

impl From<Args> for ConfigBuilder {
    fn from(args: Args) -> Self {
        if args.watch {
            warn!("The --watch flag is no longer supported, and will be ignored");
        }
        if env::var_os("WATCH_MODE").is_some() {
            warn!(
                "The WATCH_MODE environment variable is no longer supported, and will be ignored"
            );
        }

        ConfigBuilder {
            srv: SrvConfig::from(args.srv),
            postgres: parse_pg_args(args.pg, &args.connection),
            unrecognized: HashMap::new(),
        }
    }
}

async fn start(args: Args) -> io::Result<Server> {
    info!("Starting Martin v{VERSION}");

    let save_config = args.save_config.clone();
    let file_cfg = if let Some(ref cfg_filename) = args.config {
        info!("Using {}", cfg_filename.display());
        Some(read_config(cfg_filename)?)
    } else {
        info!("Config file is not specified, auto-detecting sources");
        None
    };
    let mut builder = ConfigBuilder::from(args);
    if let Some(file_cfg) = file_cfg {
        builder.merge(file_cfg);
    }
    let id_resolver = IdResolver::new(RESERVED_KEYWORDS);
    let mut config = builder.finalize()?;
    let sources = config.resolve(id_resolver).await?;

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
            File::create(file_name)?.write_all(yaml.as_bytes())?;
        }
    } else if config
        .postgres
        .iter()
        .any(|v| v.iter().any(PgConfig::is_autodetect))
    {
        info!("Martin has been configured with automatic settings.");
        info!("Use --save-config to save or print Martin configuration.");
    }

    let (server, listen_addresses) = server::new(config.srv, sources);
    info!("Martin has been started on {listen_addresses}.");
    info!("Use http://{listen_addresses}/catalog to get the list of available sources.");

    Ok(server)
}

#[actix_web::main]
async fn main() -> io::Result<()> {
    let env = env_logger::Env::default().filter_or(env_logger::DEFAULT_FILTER_ENV, "martin=info");
    env_logger::Builder::from_env(env).init();
    match start(Args::parse()).await {
        Ok(server) => server.await,
        Err(error) => {
            error!("{error}");
            std::process::exit(-1);
        }
    }
}
