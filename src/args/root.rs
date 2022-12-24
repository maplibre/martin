use crate::args::environment::{Env, SystemEnv};
use crate::args::pg::{parse_pg_args, PgArgs};
use crate::args::srv::SrvArgs;
use crate::config::Config;
use crate::srv::config::SrvConfig;
use crate::{Error, Result};
use clap::Parser;
use log::warn;
use std::path::PathBuf;

#[derive(Parser, Debug, PartialEq, Default)]
#[command(about, version)]
pub struct Args {
    #[command(flatten)]
    pub meta: MetaArgs,
    #[command(flatten)]
    pub srv: SrvArgs,
    #[command(flatten)]
    pub pg: Option<PgArgs>,
}

// None of these params will be transferred to the config
#[derive(Parser, Debug, Clone, PartialEq, Default)]
#[command(about, version)]
pub struct MetaArgs {
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
    /// Database connection strings
    pub connection: Vec<String>,
}

impl TryFrom<Args> for Config {
    type Error = Error;

    fn try_from(args: Args) -> Result<Self> {
        parse_args(&SystemEnv::default(), args)
    }
}

fn parse_args(env: &impl Env, args: Args) -> Result<Config> {
    if args.meta.watch {
        warn!("The --watch flag is no longer supported, and will be ignored");
    }
    if env.var_os("WATCH_MODE").is_some() {
        warn!("The WATCH_MODE environment variable is no longer supported, and will be ignored");
    }

    if args.meta.config.is_some() {
        if args.pg.is_some() || !args.meta.connection.is_empty() {
            return Err(Error::ConfigAndConnectionsError);
        }
        return Ok(Config {
            srv: SrvConfig::from(args.srv),
            ..Default::default()
        });
    }

    let pg = args.pg.unwrap_or_default();
    Ok(Config {
        srv: SrvConfig::from(args.srv),
        postgres: parse_pg_args(env, &pg, &args.meta.connection),
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::FauxEnv;

    fn parse(args: &[&str]) -> Result<(Config, MetaArgs)> {
        let args = Args::parse_from(args);
        let meta = args.meta.clone();
        parse_args(&FauxEnv::default(), args).map(|v| (v, meta))
    }

    #[test]
    fn cli_no_args() {
        let args = parse(&["martin"]).unwrap();
        let expected = (Config::default(), MetaArgs::default());
        assert_eq!(args, expected);
    }

    #[test]
    fn cli_with_config() {
        let args = parse(&["martin", "--config", "c.toml"]).unwrap();
        let meta = MetaArgs {
            config: Some(PathBuf::from("c.toml")),
            ..Default::default()
        };
        assert_eq!(args, (Config::default(), meta));

        let args = parse(&["martin", "--config", "c.toml", "--save-config", "s.toml"]).unwrap();
        let meta = MetaArgs {
            config: Some(PathBuf::from("c.toml")),
            save_config: Some(PathBuf::from("s.toml")),
            ..Default::default()
        };
        assert_eq!(args, (Config::default(), meta));

        let args = parse(&["martin", "connection"]).unwrap();
        let meta = MetaArgs {
            connection: vec!["connection".to_string()],
            ..Default::default()
        };
        assert_eq!(args, (Config::default(), meta));
    }

    #[test]
    fn cli_bad_arguments() {
        for params in [
            ["martin", "--config", "c.toml", "--tmp"].as_slice(),
            ["martin", "--config", "c.toml", "-c", "t.toml"].as_slice(),
        ] {
            let res = Args::try_parse_from(params);
            assert!(res.is_err(), "Expected error, got: {res:?} for {params:?}");
        }
    }

    #[test]
    fn cli_bad_parsed_arguments() {
        let args = Args::parse_from(["martin", "--config", "c.toml", "connection"]);
        let err = parse_args(&FauxEnv::default(), args).unwrap_err();
        assert!(matches!(err, Error::ConfigAndConnectionsError));
    }
}
