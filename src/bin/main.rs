use actix_web::dev::Server;
use clap::Parser;
use log::{error, info, warn};
use martin::config::{read_config, ConfigBuilder};
use martin::pg::config::{PgArgs, PgConfigBuilder};
use martin::pg::db::configure_db_sources;
use martin::srv::config::{SrvArgs, SrvConfigBuilder};
use martin::srv::server;
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
            pg: PgConfigBuilder::from((args.pg, args.connection)),
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
    let mut config = builder.finalize()?;

    let (sources, _) = configure_db_sources(&mut config).await?;
    if save_config.is_some() || config.pg.use_dynamic_sources {
        let yaml = serde_yaml::to_string(&config).expect("Unable to serialize config");
        let file_name = save_config.as_deref().unwrap_or("-");
        if file_name == "-" {
            info!("Martin has been configured with these settings.");
            info!("You can copy/paste it into a config file, and use --config.");
            println!("\n\n{yaml}\n");
        } else {
            info!("Saving config to {file_name}");
            File::create(file_name)?.write_all(yaml.as_bytes())?;
        }
    }

    let listen_addresses = config.srv.listen_addresses.clone();
    let server = server::new(config, sources);
    info!("Martin has been started on {listen_addresses}.");
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
