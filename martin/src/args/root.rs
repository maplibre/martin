use std::path::PathBuf;

use clap::Parser;
use log::warn;

use crate::args::connections::Arguments;
use crate::args::environment::Env;
use crate::args::pg::PgArgs;
use crate::args::srv::SrvArgs;
use crate::args::State::{Ignore, Share, Take};
use crate::config::Config;
use crate::file_config::FileConfigEnum;
use crate::{Error, Result};

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
    /// Connection strings, e.g. postgres://... or /path/to/files
    pub connection: Vec<String>,
    /// Export a directory with SVG files as a sprite source. Can be specified multiple times.
    #[arg(short, long)]
    pub sprite: Vec<PathBuf>,
}

impl Args {
    pub fn merge_into_config<'a>(self, config: &mut Config, env: &impl Env<'a>) -> Result<()> {
        if self.meta.watch {
            warn!("The --watch flag is no longer supported, and will be ignored");
        }
        if env.has_unused_var("WATCH_MODE") {
            warn!("The WATCH_MODE env variable is no longer supported, and will be ignored");
        }
        if self.meta.config.is_some() && !self.meta.connection.is_empty() {
            return Err(Error::ConfigAndConnectionsError(self.meta.connection));
        }

        self.srv.merge_into_config(&mut config.srv);

        let mut cli_strings = Arguments::new(self.meta.connection);
        let pg_args = self.pg.unwrap_or_default();
        if config.postgres.is_none() {
            config.postgres = pg_args.into_config(&mut cli_strings, env);
        } else {
            // config was loaded from a file, we can only apply a few CLI overrides to it
            pg_args.override_config(&mut config.postgres, env);
        }

        if !cli_strings.is_empty() {
            config.pmtiles = parse_file_args(&mut cli_strings, "pmtiles");
        }

        if !cli_strings.is_empty() {
            config.mbtiles = parse_file_args(&mut cli_strings, "mbtiles");
        }

        if !self.meta.sprite.is_empty() {
            config.sprites = FileConfigEnum::new(self.meta.sprite);
        }

        cli_strings.check()
    }
}

pub fn parse_file_args(cli_strings: &mut Arguments, extension: &str) -> FileConfigEnum {
    let paths = cli_strings.process(|v| match PathBuf::try_from(v) {
        Ok(v) => {
            if v.is_dir() {
                Share(v)
            } else if v.is_file() && v.extension().map_or(false, |e| e == extension) {
                Take(v)
            } else {
                Ignore
            }
        }
        Err(_) => Ignore,
    });

    FileConfigEnum::new(paths)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pg::PgConfig;
    use crate::test_utils::{some, FauxEnv};
    use crate::utils::OptOneMany;

    fn parse(args: &[&str]) -> Result<(Config, MetaArgs)> {
        let args = Args::parse_from(args);
        let meta = args.meta.clone();
        let mut config = Config::default();
        args.merge_into_config(&mut config, &FauxEnv::default())?;
        Ok((config, meta))
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

        let args = parse(&["martin", "postgres://connection"]).unwrap();
        let cfg = Config {
            postgres: OptOneMany::One(PgConfig {
                connection_string: some("postgres://connection"),
                ..Default::default()
            }),
            ..Default::default()
        };
        let meta = MetaArgs {
            connection: vec!["postgres://connection".to_string()],
            ..Default::default()
        };
        assert_eq!(args, (cfg, meta));
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
        let args = Args::parse_from(["martin", "--config", "c.toml", "postgres://a"]);

        let env = FauxEnv::default();
        let mut config = Config::default();
        let err = args.merge_into_config(&mut config, &env).unwrap_err();
        assert!(matches!(err, crate::Error::ConfigAndConnectionsError(..)));
    }

    #[test]
    fn cli_unknown_con_str() {
        let args = Args::parse_from(["martin", "foobar"]);

        let env = FauxEnv::default();
        let mut config = Config::default();
        let err = args.merge_into_config(&mut config, &env).unwrap_err();
        let bad = vec!["foobar".to_string()];
        assert!(matches!(err, crate::Error::UnrecognizableConnections(v) if v == bad));
    }
}
