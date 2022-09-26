use std::{env, io};

use actix_web::dev::Server;
use clap::Parser;
use log::{error, info, warn};
use martin::config::{read_config, Config, ConfigBuilder};
use martin::db::{assert_postgis_version, get_connection, setup_connection_pool, Pool};
use martin::function_source::get_function_sources;
use martin::table_source::get_table_sources;
use martin::{prettify_error, server};
use serde::Deserialize;

const VERSION: &str = env!("CARGO_PKG_VERSION");
const REQUIRED_POSTGIS_VERSION: &str = ">= 2.4.0";

#[derive(Parser, Debug, Deserialize)]
#[command(about, version)]
pub struct Args {
    /// Database connection string
    pub connection: Option<String>,
    /// Path to config file.
    #[arg(short, long)]
    pub config: Option<String>,
    /// Loads trusted root certificates from a file. The file should contain a sequence of PEM-formatted CA certificates.
    #[arg(long)]
    pub ca_root_file: Option<String>,
    /// Trust invalid certificates. This introduces significant vulnerabilities, and should only be used as a last resort.
    #[arg(long)]
    pub danger_accept_invalid_certs: bool,
    /// If a spatial table has SRID 0, then this default SRID will be used as a fallback.
    #[arg(short, long)]
    pub default_srid: Option<i32>,
    #[arg(short, long,
    help = format ! ("Connection keep alive timeout. [DEFAULT: {}]", ConfigBuilder::KEEP_ALIVE_DEFAULT))]
    pub keep_alive: Option<usize>,
    #[arg(short, long,
    help = format ! ("The socket address to bind. [DEFAULT: {}]", ConfigBuilder::LISTEN_ADDRESSES_DEFAULT))]
    pub listen_addresses: Option<String>,
    #[arg(short, long,
    help = format ! ("Maximum connections pool size [DEFAULT: {}]", ConfigBuilder::POOL_SIZE_DEFAULT))]
    pub pool_size: Option<u32>,
    /// Scan for new sources on sources list requests
    #[arg(short, long, hide = true)]
    pub watch: bool,
    /// Number of web server workers
    #[arg(short = 'W', long)]
    pub workers: Option<usize>,
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
            connection_string: args.connection.or_else(|| {
                env::var_os("DATABASE_URL").and_then(|connection| connection.into_string().ok())
            }),
            ca_root_file: args.ca_root_file.or_else(|| {
                env::var_os("CA_ROOT_FILE").and_then(|connection| connection.into_string().ok())
            }),
            danger_accept_invalid_certs: if args.danger_accept_invalid_certs
                || env::var_os("DANGER_ACCEPT_INVALID_CERTS").is_some()
            {
                Some(true)
            } else {
                None
            },
            default_srid: args.default_srid.or_else(|| {
                env::var_os("DEFAULT_SRID").and_then(|srid| {
                    srid.into_string()
                        .ok()
                        .and_then(|srid| srid.parse::<i32>().ok())
                })
            }),
            keep_alive: args.keep_alive,
            listen_addresses: args.listen_addresses,
            pool_size: args.pool_size,
            worker_processes: args.workers,
            table_sources: None,
            function_sources: None,
        }
    }
}

async fn setup_from_config(file_name: String) -> io::Result<(Config, Pool)> {
    let config = read_config(&file_name).map_err(|e| prettify_error!(e, "Can't read config"))?;
    let pool = setup_connection_pool(
        &config.connection_string,
        &config.ca_root_file,
        Some(config.pool_size),
        config.danger_accept_invalid_certs,
    )
    .await
    .map_err(|e| prettify_error!(e, "Can't setup connection pool"))?;

    if let Some(table_sources) = &config.table_sources {
        for table_source in table_sources.values() {
            info!(
                r#"Found "{}" table source with "{}" column ({}, SRID={})"#,
                table_source.id,
                table_source.geometry_column,
                table_source
                    .geometry_type
                    .as_ref()
                    .unwrap_or(&"null".to_string()),
                table_source.srid
            );
        }
    }
    if let Some(function_sources) = &config.function_sources {
        for function_source in function_sources.values() {
            info!("Found {} function source", function_source.id);
        }
    }
    info!("Connected to {}", config.connection_string);
    Ok((config, pool))
}

async fn setup_from_args(args: Args) -> io::Result<(Config, Pool)> {
    let connection_string = args.connection.clone().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::Other,
            "Database connection string is not set",
        )
    })?;

    info!("Connecting to database");
    let pool = setup_connection_pool(
        &connection_string,
        &args.ca_root_file,
        args.pool_size,
        args.danger_accept_invalid_certs,
    )
    .await
    .map_err(|e| prettify_error!(e, "Can't setup connection pool"))?;

    let mut config = ConfigBuilder::from(args);
    {
        info!("Scanning database");
        let mut connection = get_connection(&pool).await?;
        config.table_sources = Some(get_table_sources(&mut connection, config.default_srid).await?);
        config.function_sources = Some(get_function_sources(&mut connection).await?);
    }
    let config = config.finalize();
    Ok((config, pool))
}

async fn start(args: Args) -> io::Result<Server> {
    info!("Starting Martin v{VERSION}");
    let (config, pool) = match args.config {
        Some(config_file_name) => {
            info!("Using {config_file_name}");
            setup_from_config(config_file_name).await?
        }
        None => {
            info!("Config file is not specified");
            setup_from_args(args).await?
        }
    };
    assert_postgis_version(REQUIRED_POSTGIS_VERSION, &pool).await?;
    let listen_addresses = config.listen_addresses.clone();
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
