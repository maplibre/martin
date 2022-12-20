use crate::args::pg::{parse_pg_args, PgArgs};
use crate::args::srv::SrvArgs;
use crate::config::Config;
use crate::srv::config::SrvConfig;
use crate::Error;
use clap::Parser;
use log::warn;
use std::env;
use std::path::PathBuf;

pub mod pg;
pub mod srv;

#[derive(Parser, Debug)]
#[command(about, version)]
pub struct Args {
    // config may need a   conflicts_with = "SourcesArgs"
    // see https://github.com/clap-rs/clap/discussions/4562
    /// Path to config file. If set, no tile source-related parameters are allowed.
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
    sources: Option<SourcesArgs>,
}

#[derive(Parser, Debug, Default)]
#[command(about, version)]
pub struct SourcesArgs {
    /// Database connection strings
    pub connection: Vec<String>,
    #[command(flatten)]
    pg: PgArgs,
}

impl TryFrom<Args> for Config {
    type Error = Error;

    fn try_from(args: Args) -> crate::Result<Self> {
        if args.watch {
            warn!("The --watch flag is no longer supported, and will be ignored");
        }
        if env::var_os("WATCH_MODE").is_some() {
            warn!(
                "The WATCH_MODE environment variable is no longer supported, and will be ignored"
            );
        }

        if args.config.is_some() {
            if args.sources.is_some() {
                return Err(Error::ConfigAndConnectionsError);
            }
            return Ok(Config {
                srv: SrvConfig::from(args.srv),
                ..Default::default()
            });
        }

        let sources = args.sources.unwrap_or_default();
        Ok(Config {
            srv: SrvConfig::from(args.srv),
            postgres: parse_pg_args(&sources.pg, &sources.connection),
            ..Default::default()
        })
    }
}
