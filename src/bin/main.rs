use actix_web::dev::Server;
use clap::Parser;
use log::{error, info, warn};
use martin::config::{read_config, ConfigBuilder};
use martin::pg::config::{PgArgs, PgConfigBuilder};
use martin::pg::db::configure_db_sources;
use martin::srv::config::{SrvArgs, SrvConfigBuilder};
use martin::srv::server;
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

    let mut config = if let Some(ref config_file_name) = args.config {
        info!("Using {config_file_name}");
        let cfg = read_config(config_file_name)?;
        let mut builder = ConfigBuilder::from(args);
        builder.merge(cfg);
        builder.finalize()?
    } else {
        info!("Config file is not specified");
        ConfigBuilder::from(args).finalize()?
    };

    let pool = configure_db_sources(&mut config).await?;
    let listen_addresses = config.srv.listen_addresses.clone();
    let server = server::new(pool, config);

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
