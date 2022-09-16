use std::{env, io};

use actix_web::dev::Server;
use clap::Parser;
use log::{error, info, warn};
use martin::config::{read_config, Config, ConfigBuilder};
use martin::db::{check_postgis_version, get_connection, setup_connection_pool, Pool};
use martin::function_source::get_function_sources;
use martin::table_source::get_table_sources;
use martin::{prettify_error, server};
use serde::Deserialize;

const VERSION: &str = env!("CARGO_PKG_VERSION");
const REQUIRED_POSTGIS_VERSION: &str = ">= 2.4.0";

#[derive(Parser, Debug, Deserialize)]
#[clap(about, version)]
pub struct Args {
    /// Path to config file.
    #[clap(short, long)]
    pub config: Option<String>,
    /// Loads trusted root certificates from a file. The file should contain a sequence of PEM-formatted CA certificates.
    #[clap(long)]
    pub ca_root_file: Option<String>,
    /// Trust invalid certificates. This introduces significant vulnerabilities, and should only be used as a last resort.
    #[clap(long)]
    pub danger_accept_invalid_certs: bool,
    /// If a spatial table has SRID 0, then this default SRID will be used as a fallback.
    #[clap(short, long)]
    pub default_srid: Option<i32>,
    // Must match ConfigBuilder::KEEP_ALIVE_DEFAULT
    /// Connection keep alive timeout. [DEFAULT: 75]
    #[clap(short, long)]
    pub keep_alive: Option<usize>,
    // Must match ConfigBuilder::LISTEN_ADDRESSES_DEFAULT
    /// The socket address to bind. [DEFAULT: 0.0.0.0:3000]
    #[clap(short, long)]
    pub listen_addresses: Option<String>,
    // Must match ConfigBuilder::POOL_SIZE_DEFAULT
    /// Maximum connections pool size [DEFAULT: 20]
    #[clap(short, long)]
    pub pool_size: Option<u32>,
    /// Scan for new sources on sources list requests
    #[clap(short, long, hide = true)]
    pub watch: bool,
    /// Number of web server workers
    #[clap(short = 'W', long)]
    pub workers: Option<usize>,
    /// Database connection string
    pub connection: Option<String>,
}

impl Args {
    fn to_config(self) -> ConfigBuilder {
        ConfigBuilder {
            ca_root_file: self.ca_root_file,
            danger_accept_invalid_certs: if self.danger_accept_invalid_certs {
                Some(true)
            } else {
                None
            },
            default_srid: self.default_srid,
            keep_alive: self.keep_alive,
            listen_addresses: self.listen_addresses,
            pool_size: self.pool_size,
            worker_processes: self.workers,
            connection_string: self.connection,
            table_sources: None,
            function_sources: None,
        }
    }
}

pub async fn generate_config(args: Args, pool: &Pool) -> io::Result<Config> {
    // let connection_string = args.arg_connection.clone().ok_or_else(|| {
    //     io::Error::new(
    //         io::ErrorKind::Other,
    //         "Database connection string is not set",
    //     )
    // })?;
    let connection_string = args.connection.clone();

    let mut connection = get_connection(pool).await?;
    let table_sources = get_table_sources(&mut connection, &args.default_srid).await?;
    let function_sources = get_function_sources(&mut connection).await?;

    let config = ConfigBuilder {
        ca_root_file: None,
        danger_accept_invalid_certs: if args.danger_accept_invalid_certs { Some(true) } else { None },
        default_srid: args.default_srid,
        keep_alive: args.keep_alive,
        listen_addresses: args.listen_addresses,
        pool_size: args.pool_size,
        worker_processes: args.workers,
        connection_string,
        table_sources: Some(table_sources),
        function_sources: Some(function_sources),
    };

    let config = config.finalize();
    Ok(config)
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

    info!("Scanning database");
    let config = generate_config(args, &pool)
        .await
        .map_err(|e| prettify_error!(e, "Can't generate config"))?;

    Ok((config, pool))
}

fn parse_env(args: Args) -> Args {
    let connection = args.connection.or_else(|| {
        env::var_os("DATABASE_URL").and_then(|connection| connection.into_string().ok())
    });

    let default_srid = args.default_srid.or_else(|| {
        env::var_os("DEFAULT_SRID").and_then(|srid| {
            srid.into_string()
                .ok()
                .and_then(|srid| srid.parse::<i32>().ok())
        })
    });

    let ca_root_file = args.ca_root_file.or_else(|| {
        env::var_os("CA_ROOT_FILE").and_then(|connection| connection.into_string().ok())
    });

    let danger_accept_invalid_certs =
        args.danger_accept_invalid_certs || env::var_os("DANGER_ACCEPT_INVALID_CERTS").is_some();

    if args.watch {
        warn!("The --watch flag is no longer supported, and will be ignored");
    }
    if env::var_os("WATCH_MODE").is_some() {
        warn!("The WATCH_MODE environment variable is no longer supported, and will be ignored");
    }

    Args {
        connection,
        default_srid,
        ca_root_file,
        danger_accept_invalid_certs,
        ..args
    }
}

async fn start(args: Args) -> io::Result<Server> {
    info!("Starting martin v{VERSION}");

    let (config, pool) = match args.config {
        Some(config_file_name) => {
            info!("Using {config_file_name}");
            setup_from_config(config_file_name).await?
        }
        None => {
            info!("Config is not set");
            setup_from_args(args).await?
        }
    };

    let matches = check_postgis_version(REQUIRED_POSTGIS_VERSION, &pool)
        .await
        .map_err(|e| prettify_error!(e, "Can't check PostGIS version"))?;

    if !matches {
        std::process::exit(-1);
    }

    let listen_addresses = config.listen_addresses.clone();
    let server = server::new(pool, config);
    info!("Martin has been started on {listen_addresses}.");

    Ok(server)
}

fn print_type_of<T>(_: &T) {
    println!("{}", std::any::type_name::<T>())
}

#[actix_web::main]
async fn main() -> io::Result<()> {
    let env = env_logger::Env::default().filter_or(env_logger::DEFAULT_FILTER_ENV, "martin=info");
    env_logger::Builder::from_env(env).init();

    let args: Args = Args::parse();
    println!("{:?}", &args);
    let parse: ConfigBuilder = args.to_config();
    print_type_of(&parse);
    println!("{:?}", &parse);
    // parse.map(|args| {
    //     let args = parse_env(args);
    //     let runner = start(args)?;
    //     runner.run();
    // });
    // let clap_matches = Args::command().ignore_errors(true).get_matches();
    // print_type_of(&clap_matches); // clap::parse::matches::arg_matches::ArgMatches

    // let mut v = Viperus::new();
    // v.load_clap(clap_matches).unwrap();
    // let config = clap_matches.value_of("config");
    // if config {
    //     v.load_file(config, Format::YAML).unwrap();
    // }
    // debug!("final {:?}", v);
    //
    // // let x = args.value_of("foo");
    // // let args = Docopt::new(USAGE)
    // //     .and_then(|d| d.help(false).deserialize::<Args>())
    // //     .map_err(prettify_error("Can't parse CLI arguments".to_owned()))?;
    //
    // // let args = parse_env(args);
    // // let args = Args::parse();
    // let args = Args {
    //     config: v.get("config").unwrap(),
    //     keep_alive: v.get("keep_alive").unwrap(),
    //     listen_addresses: v.get("listen_addresses").unwrap(),
    //     default_srid: v.get("default_srid").unwrap(),
    //     pool_size: v.get("pool_size").unwrap(),
    //     workers: v.get("workers").unwrap(),
    //     ca_root_file: v.get("ca_root_file").unwrap(),
    //     danger_accept_invalid_certs: v.get("danger_accept_invalid_certs").unwrap(),
    //     connection: v.get("connection").unwrap(),
    // };
    //
    //
    // if args.danger_accept_invalid_certs {
    //     warn!("Danger accept invalid certs enabled. You should think very carefully before using this option. If invalid certificates are trusted, any certificate for any site will be trusted for use. This includes expired certificates. This introduces significant vulnerabilities, and should only be used as a last resort.");
    // }
    //
    // let server = match start(args) {
    //     Ok(server) => server,
    //     Err(error) => {
    //         error!("{error}");
    //         std::process::exit(-1);
    //     }
    // };
    //
    // server.run()
    // let server = match start(args) {
    //     Ok(server) => server,
    //     Err(error) => {
    //         error!("{error}");
    //         std::process::exit(-1);
    //     }
    // };

    // match start(args).await {
    //     Ok(server) => server.await,
    //     Err(error) => {
    //         error!("{error}");
    //         std::process::exit(-1);
    //     }
    // }
    Ok(())
}
