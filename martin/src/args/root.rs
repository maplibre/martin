use std::path::PathBuf;

use clap::Parser;
use clap::builder::Styles;
use clap::builder::styling::AnsiColor;
use log::warn;
#[cfg(feature = "fonts")]
use martin_core::config::OptOneMany;
use martin_core::config::env::Env;

use crate::MartinError::ConfigAndConnectionsError;
use crate::MartinResult;
use crate::args::connections::Arguments;
use crate::args::srv::SrvArgs;
use crate::config::Config;
#[cfg(any(
    feature = "cog",
    feature = "mbtiles",
    feature = "pmtiles",
    feature = "sprites",
    feature = "styles",
))]
use crate::file_config::FileConfigEnum;

/// Defines the styles used for the CLI help output.
const HELP_STYLES: Styles = Styles::styled()
    .header(AnsiColor::Blue.on_default().bold())
    .usage(AnsiColor::Blue.on_default().bold())
    .literal(AnsiColor::White.on_default())
    .placeholder(AnsiColor::Green.on_default());

#[derive(Parser, Debug, PartialEq, Default)]
#[command(
    about,
    version,
    after_help = "Use RUST_LOG environment variable to control logging level, e.g. RUST_LOG=debug or RUST_LOG=martin=debug. See https://docs.rs/env_logger/latest/env_logger/index.html#enabling-logging for more information.",
    styles = HELP_STYLES
)]
pub struct Args {
    #[command(flatten)]
    pub meta: MetaArgs,
    #[command(flatten)]
    pub extras: ExtraArgs,
    #[command(flatten)]
    pub srv: SrvArgs,
    #[cfg(feature = "postgres")]
    #[command(flatten)]
    pub pg: Option<crate::args::pg::PgArgs>,
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
    /// Connection strings, e.g. `postgres://...` or `/path/to/files`
    pub connection: Vec<String>,
}

#[derive(Parser, Debug, Clone, PartialEq, Default)]
#[command()]
pub struct ExtraArgs {
    /// Export a directory with SVG files as a sprite source. Can be specified multiple times.
    #[arg(short = 's', long)]
    #[cfg(feature = "sprites")]
    pub sprite: Vec<PathBuf>,
    /// Export a font file or a directory with font files as a font source (recursive). Can be specified multiple times.
    #[arg(short, long)]
    #[cfg(feature = "fonts")]
    pub font: Vec<PathBuf>,
    /// Export a style file or a directory with style files as a style source (recursive). Can be specified multiple times.
    #[arg(short = 'S', long)]
    #[cfg(feature = "styles")]
    pub style: Vec<PathBuf>,
}

impl Args {
    pub fn merge_into_config<'a>(
        self,
        config: &mut Config,
        #[allow(unused_variables)] env: &impl Env<'a>,
    ) -> MartinResult<()> {
        if self.srv.watch {
            warn!("The --watch flag is no longer supported, and will be ignored");
        }
        if self.meta.config.is_some() && !self.meta.connection.is_empty() {
            return Err(ConfigAndConnectionsError(self.meta.connection));
        }

        if self.srv.cache_size.is_some() {
            config.cache_size_mb = self.srv.cache_size;
        }

        self.srv.merge_into_config(&mut config.srv);

        #[allow(unused_mut)]
        let mut cli_strings = Arguments::new(self.meta.connection);

        #[cfg(feature = "postgres")]
        {
            let pg_args = self.pg.unwrap_or_default();
            if config.postgres.is_none() {
                config.postgres = pg_args.into_config(&mut cli_strings, env);
            } else {
                // config was loaded from a file, we can only apply a few CLI overrides to it
                pg_args.override_config(&mut config.postgres, env);
            }
        }

        #[cfg(feature = "pmtiles")]
        if !cli_strings.is_empty() {
            config.pmtiles = parse_file_args(&mut cli_strings, &["pmtiles"], true);
        }

        #[cfg(feature = "mbtiles")]
        if !cli_strings.is_empty() {
            config.mbtiles = parse_file_args(&mut cli_strings, &["mbtiles"], false);
        }

        #[cfg(feature = "cog")]
        if !cli_strings.is_empty() {
            config.cog = parse_file_args(&mut cli_strings, &["tif", "tiff"], false);
        }

        #[cfg(feature = "styles")]
        if !self.extras.style.is_empty() {
            config.styles = FileConfigEnum::new(self.extras.style);
        }

        #[cfg(feature = "sprites")]
        if !self.extras.sprite.is_empty() {
            config.sprites = FileConfigEnum::new(self.extras.sprite);
        }

        #[cfg(feature = "fonts")]
        if !self.extras.font.is_empty() {
            config.fonts = OptOneMany::new(self.extras.font);
        }

        cli_strings.check()
    }
}

/// Check if a string is a valid [`url::Url`] with a specified extension.
#[cfg(any(feature = "cog", feature = "mbtiles", feature = "pmtiles"))]
fn is_url(s: &str, extension: &[&str]) -> bool {
    let Ok(url) = url::Url::parse(s) else {
        return false;
    };
    match url.scheme() {
        "s3" => url.path().split('/').any(|segment| {
            segment
                .rsplit('.')
                .next()
                .is_some_and(|ext| extension.contains(&ext))
        }),
        "http" | "https" => url
            .path()
            .rsplit('.')
            .next()
            .is_some_and(|ext| extension.contains(&ext)),
        _ => false,
    }
}

#[cfg(any(feature = "cog", feature = "mbtiles", feature = "pmtiles"))]
pub fn parse_file_args<T: crate::file_config::ConfigExtras>(
    cli_strings: &mut Arguments,
    extensions: &[&str],
    allow_url: bool,
) -> FileConfigEnum<T> {
    use crate::args::State::{Ignore, Share, Take};

    let paths = cli_strings.process(|s| {
        let path = PathBuf::from(s);
        if allow_url && is_url(s, extensions) {
            Take(path)
        } else if path.is_dir() {
            Share(path)
        } else if path.is_file()
            && extensions.iter().any(|&expected_ext| {
                path.extension()
                    .is_some_and(|actual_ext| actual_ext == expected_ext)
            })
        {
            Take(path)
        } else {
            Ignore
        }
    });

    FileConfigEnum::new(paths)
}

#[cfg(test)]
mod tests {

    use insta::assert_yaml_snapshot;
    use martin_core::config::env::FauxEnv;

    use super::*;
    use crate::MartinError::UnrecognizableConnections;
    use crate::args::PreferredEncoding;

    fn parse(args: &[&str]) -> MartinResult<(Config, MetaArgs)> {
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

    #[cfg(feature = "postgres")]
    #[test]
    fn cli_with_config() {
        use martin_core::config::OptOneMany;

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
            postgres: OptOneMany::One(crate::pg::PgConfig {
                connection_string: Some("postgres://connection".to_string()),
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
    fn cli_encoding_arguments() {
        let config1 = parse(&["martin", "--preferred-encoding", "brotli"]);
        let config2 = parse(&["martin", "--preferred-encoding", "br"]);
        let config3 = parse(&["martin", "--preferred-encoding", "gzip"]);
        let config4 = parse(&["martin"]);

        assert_eq!(
            config1.unwrap().0.srv.preferred_encoding,
            Some(PreferredEncoding::Brotli)
        );
        assert_eq!(
            config2.unwrap().0.srv.preferred_encoding,
            Some(PreferredEncoding::Brotli)
        );
        assert_eq!(
            config3.unwrap().0.srv.preferred_encoding,
            Some(PreferredEncoding::Gzip)
        );
        assert_eq!(config4.unwrap().0.srv.preferred_encoding, None);
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
        assert!(matches!(err, ConfigAndConnectionsError(..)));
    }

    #[test]
    fn cli_unknown_con_str() {
        let args = Args::parse_from(["martin", "foobar"]);

        let env = FauxEnv::default();
        let mut config = Config::default();
        let err = args.merge_into_config(&mut config, &env).unwrap_err();
        let bad = vec!["foobar".to_string()];
        assert!(matches!(err, UnrecognizableConnections(v) if v == bad));
    }

    #[test]
    fn cli_multiple_extensions() {
        let args = Args::parse_from([
            "martin",
            "../tests/fixtures/pmtiles/png.pmtiles",
            "../tests/fixtures/mbtiles/json.mbtiles",
            "../tests/fixtures/cog/rgba_u8_nodata.tiff",
            "../tests/fixtures/cog/rgba_u8.tif",
        ]);

        let env = FauxEnv::default();
        let mut config = Config::default();
        let err = args.merge_into_config(&mut config, &env);
        assert!(err.is_ok());
        assert_yaml_snapshot!(config, @r#"
        pmtiles: "../tests/fixtures/pmtiles/png.pmtiles"
        mbtiles: "../tests/fixtures/mbtiles/json.mbtiles"
        cog:
          - "../tests/fixtures/cog/rgba_u8_nodata.tiff"
          - "../tests/fixtures/cog/rgba_u8.tif"
        "#);
    }

    #[test]
    fn cli_directories_propagate() {
        let args = Args::parse_from(["martin", "../tests/fixtures/"]);

        let env = FauxEnv::default();
        let mut config = Config::default();
        let err = args.merge_into_config(&mut config, &env);
        assert!(err.is_ok());
        assert_yaml_snapshot!(config, @r#"
        pmtiles: "../tests/fixtures/"
        mbtiles: "../tests/fixtures/"
        cog: "../tests/fixtures/"
        "#);
    }
}
