use actix_web::dev::Server;
use clap::Parser;
use log::{error, info, warn};
use martin::config::{read_config, Config, ConfigBuilder};
use martin::pg::config::{PgArgs, PgConfig};
use martin::pg::configurator::resolve_pg_data;
use martin::source::IdResolver;
use martin::srv::config::{SrvArgs, SrvConfigBuilder};
use martin::srv::server;
use martin::srv::server::RESERVED_KEYWORDS;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::{env, io};

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Parser, Debug)]
#[command(about, version)]
pub struct Args {
    /// Database connection string
    pub connection: Option<String>,
    /// Path to config file.
    #[arg(short, long)]
    pub config: Option<String>,
    /// Save resulting config to a file or use "-" to print to stdout.
    /// By default, only print if sources are auto-detected.
    #[arg(long)]
    pub save_config: Option<String>,
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
            srv: SrvConfigBuilder::from(args.srv),
            pg: PgConfig::from((args.pg, args.connection)),
            unrecognized: HashMap::new(),
        }
    }
}

async fn start(args: Args) -> io::Result<Server> {
    info!("Starting Martin v{VERSION}");

    let save_config = args.save_config.clone();
    let file_cfg = if let Some(ref cfg_filename) = args.config {
        info!("Using {cfg_filename}");
        Some(read_config(cfg_filename)?)
    } else {
        info!("Config file is not specified, auto-detecting sources");
        None
    };
    let mut builder = ConfigBuilder::from(args);
    if let Some(file_cfg) = file_cfg {
        builder.merge(file_cfg);
    }
    let config = builder.finalize()?;

    let id_resolver = IdResolver::new(RESERVED_KEYWORDS);
    let (sources, pg_config, _) = resolve_pg_data(config.pg, id_resolver).await?;
    let config = Config {
        pg: pg_config,
        ..config
    };

    if save_config.is_some() {
        let yaml = serde_yaml::to_string(&config).expect("Unable to serialize config");
        let file_name = save_config.as_deref().unwrap_or("-");
        if file_name == "-" {
            info!("Current system configuration:");
            println!("\n\n{yaml}\n");
        } else {
            info!("Saving config to {file_name}, use --config to load it");
            File::create(file_name)?.write_all(yaml.as_bytes())?;
        }
    } else if config.pg.run_autodiscovery {
        info!("Martin has been configured with automatic settings.");
        info!("Use --save-config to save or print Martin configuration.");
    }

    let listen_addresses = config.srv.listen_addresses.clone();
    let server = server::new(config.srv, sources);
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
