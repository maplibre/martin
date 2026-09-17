use std::num::NonZeroUsize;

use martin_core::tiles::postgres::RetryTimeout;
use tracing::{info, warn};

use super::bounds::BoundsCalcType;
use super::connections::Arguments;
use super::connections::State::{Ignore, Take};
use crate::config::file::postgres::{
    DEFAULT_POOL_SIZE, DEFAULT_RELOAD_INTERVAL, PostgresConfig, PostgresSslCerts,
};
use crate::config::file::{CachePolicy, UnrecognizedValues};
use crate::config::primitives::env::Env;
use crate::config::primitives::{OptBoolObj, OptOneMany};

#[derive(clap::Args, Debug, PartialEq, Eq, Default)]
#[command(about, version)]
pub struct PostgresArgs {
    /// Specify how bounds should be computed for the spatial PG tables. [DEFAULT: quick]
    #[arg(short = 'b', long)]
    pub auto_bounds: Option<BoundsCalcType>,
    /// Loads trusted root certificates from a file. The file should contain a sequence of PEM-formatted CA certificates.
    #[arg(long)]
    pub ca_root_file: Option<std::path::PathBuf>,
    /// If a spatial PG table has SRID 0, then this default SRID will be used as a fallback.
    #[arg(short, long)]
    pub default_srid: Option<i32>,
    #[arg(help = format!("Maximum Postgres connections pool size [DEFAULT: {DEFAULT_POOL_SIZE}]"), short, long)]
    pub pool_size: Option<NonZeroUsize>,
    /// How long the first PostgreSQL connection is retried before startup fails, a duration like `30s` or `infinite`. [DEFAULT: 30s]
    #[arg(long)]
    pub pg_retry_timeout: Option<RetryTimeout>,
    /// Limit the number of geo features per tile.
    ///
    /// If the source table has more features than set here, they will not be included in the tile and the result will look "cut off"/incomplete.
    /// This feature allows to put a maximum latency bound on tiles with extreme amount of detail at the cost of not returning all data.
    /// It is sensible to set this limit if you have user generated/untrusted geodata, e.g. a lot of data points at [Null Island](https://en.wikipedia.org/wiki/Null_Island).
    ///
    /// Can be either a positive integer or unlimited if omitted.
    #[arg(short, long)]
    pub max_feature_count: Option<usize>,
    /// A file with a client SSL certificate.
    #[arg(long)]
    pub ssl_cert: Option<std::path::PathBuf>,
    /// A file with the key for the client SSL certificate.
    #[arg(long)]
    pub ssl_key: Option<std::path::PathBuf>,
}

impl PostgresArgs {
    pub fn into_config(
        self,
        cli_strings: &mut Arguments,
        env: &impl Env,
    ) -> OptOneMany<PostgresConfig> {
        let connections = Self::extract_conn_strings(cli_strings, env);
        let default_srid = self.get_default_srid(env);
        let certs = self.get_certs(env);

        let results: Vec<_> = connections
            .into_iter()
            .map(|s| PostgresConfig {
                connection_string: Some(s),
                ssl_certificates: certs.clone(),
                default_srid,
                auto_bounds: self.auto_bounds,
                max_feature_count: self.max_feature_count,
                pool_size: self.pool_size,
                cache: CachePolicy::default(),
                retry_timeout: self.pg_retry_timeout,
                reload_interval: DEFAULT_RELOAD_INTERVAL,
                auto_publish: OptBoolObj::NoValue,
                tables: None,
                functions: None,
                #[cfg(all(feature = "mlt", feature = "_tiles"))]
                convert_to_mlt: None,
                #[cfg(all(feature = "mlt", feature = "_tiles"))]
                convert_to_mvt: None,
                unrecognized: UnrecognizedValues::default(),
            })
            .collect();

        match results.len() {
            0 => OptOneMany::NoVals,
            1 => OptOneMany::One(results.into_iter().next().expect("one result exists")),
            _ => OptOneMany::Many(results),
        }
    }

    /// Apply CLI parameters from `self` to the configuration loaded from the config file `pg_config`
    pub fn override_config(self, pg_config: &mut OptOneMany<PostgresConfig>) {
        // This ensures that if a new parameter is added to the struct, it will not be forgotten here
        let Self {
            default_srid,
            pool_size,
            pg_retry_timeout,
            auto_bounds,
            max_feature_count,
            ca_root_file,
            ssl_cert,
            ssl_key,
        } = self;

        if let Some(value) = default_srid {
            info!(
                "Overriding configured default SRID to {value} on all Postgres connections because of a CLI parameter"
            );
            pg_config.iter_mut().for_each(|c| {
                c.default_srid = default_srid;
            });
        }
        if let Some(value) = pool_size {
            info!(
                "Overriding configured pool size to {value} on all Postgres connections because of a CLI parameter"
            );
            pg_config.iter_mut().for_each(|c| {
                c.pool_size = pool_size;
            });
        }
        if let Some(value) = pg_retry_timeout {
            info!(
                "Overriding retry_timeout to {value} on all Postgres connections because of a CLI parameter"
            );
            pg_config.iter_mut().for_each(|c| {
                c.retry_timeout = pg_retry_timeout;
            });
        }
        if let Some(value) = auto_bounds {
            info!(
                "Overriding auto_bounds to {value} on all Postgres connections because of a CLI parameter"
            );
            pg_config.iter_mut().for_each(|c| {
                c.auto_bounds = auto_bounds;
            });
        }
        if let Some(value) = max_feature_count {
            info!(
                "Overriding maximum feature count to {value} on all Postgres connections because of a CLI parameter"
            );
            pg_config.iter_mut().for_each(|c| {
                c.max_feature_count = max_feature_count;
            });
        }
        if let Some(ref value) = ca_root_file {
            info!(
                "Overriding root certificate file to {} on all Postgres connections because of a CLI parameter",
                value.display()
            );
            pg_config.iter_mut().for_each(|c| {
                c.ssl_certificates.ssl_root_cert.clone_from(&ca_root_file);
            });
        }
        if let Some(ref value) = ssl_cert {
            info!(
                "Overriding client SSL certificate to {} on all Postgres connections because of a CLI parameter",
                value.display()
            );
            pg_config.iter_mut().for_each(|c| {
                c.ssl_certificates.ssl_cert.clone_from(&ssl_cert);
            });
        }
        if let Some(ref value) = ssl_key {
            info!(
                "Overriding client SSL key to {} on all Postgres connections because of a CLI parameter",
                value.display()
            );
            pg_config.iter_mut().for_each(|c| {
                c.ssl_certificates.ssl_key.clone_from(&ssl_key);
            });
        }
    }

    fn extract_conn_strings(cli_strings: &mut Arguments, env: &impl Env) -> Vec<String> {
        let mut connections = cli_strings.process(|v| {
            if is_postgres_connection_string(v) {
                Take(v.to_owned())
            } else {
                Ignore
            }
        });
        if connections.is_empty()
            && let Some(s) = env.get_env_str("DATABASE_URL")
        {
            if is_postgres_connection_string(&s) {
                info!("Using env var DATABASE_URL to connect to PostgreSQL");
                connections.push(s);
            } else {
                warn!("Environment var DATABASE_URL is not a valid postgres connection string");
            }
        }

        connections
    }

    fn get_default_srid(&self, env: &impl Env) -> Option<i32> {
        if self.default_srid.is_some() {
            return self.default_srid;
        }
        let srid = env.get_env_str("DEFAULT_SRID")?;
        match srid.parse::<i32>() {
            Ok(v) => {
                info!("Using env var DEFAULT_SRID={v} to set default SRID");
                Some(v)
            }
            Err(v) => {
                warn!("Env var DEFAULT_SRID is not a valid integer {srid}: {v}");
                None
            }
        }
    }

    fn get_certs(&self, env: &impl Env) -> PostgresSslCerts {
        let mut result = PostgresSslCerts {
            ssl_cert: self.ssl_cert.clone(),
            ssl_key: self.ssl_key.clone(),
            ssl_root_cert: self.ca_root_file.clone(),
            unrecognized: UnrecognizedValues::default(),
        };
        if result.ssl_cert.is_none() {
            result.ssl_cert = Self::parse_env_var(env, "PGSSLCERT", "ssl certificate");
        }
        if result.ssl_key.is_none() {
            result.ssl_key = Self::parse_env_var(env, "PGSSLKEY", "ssl key for certificate");
        }
        if result.ssl_root_cert.is_none() {
            result.ssl_root_cert = Self::parse_env_var(env, "PGSSLROOTCERT", "root certificate(s)");
        }

        result
    }

    fn parse_env_var(env: &impl Env, env_var: &str, info: &str) -> Option<std::path::PathBuf> {
        let path = env.var_os(env_var).map(std::path::PathBuf::from);
        if let Some(p) = &path {
            let p = p.display();
            info!("Using env {env_var}={p} to load {info}");
        }
        path
    }
}

#[must_use]
fn is_postgres_connection_string(s: &str) -> bool {
    s.starts_with("postgresql://") || s.starts_with("postgres://")
}

#[cfg(test)]
mod tests {
    use std::assert_matches;
    use std::ffi::OsString;
    use std::path::PathBuf;

    use super::*;
    use crate::config::args::ArgsError;
    use crate::config::primitives::env::FauxEnv;

    #[test]
    fn extracts_conn_strings() {
        let mut args = Arguments::new(vec![
            "postgresql://localhost:5432".to_owned(),
            "postgres://localhost:5432".to_owned(),
            "mysql://localhost:3306".to_owned(),
        ]);
        assert_eq!(
            PostgresArgs::extract_conn_strings(&mut args, &FauxEnv::default()),
            vec!["postgresql://localhost:5432", "postgres://localhost:5432"]
        );
        assert_matches!(
            args.check(),
            Err(ArgsError::UnrecognizableConnections(v)) if v == vec!["mysql://localhost:3306"]
        );
    }

    #[test]
    fn extract_conn_strings_from_env() {
        let mut args = Arguments::new(vec![]);
        let env = FauxEnv(
            vec![(
                "DATABASE_URL",
                OsString::from("postgresql://localhost:5432"),
            )]
            .into_iter()
            .collect(),
        );
        let strings = PostgresArgs::extract_conn_strings(&mut args, &env);
        assert_eq!(strings, vec!["postgresql://localhost:5432"]);
        args.check().unwrap();
    }

    #[test]
    fn merge_into_config() {
        let mut args = Arguments::new(vec!["postgres://localhost:5432".to_owned()]);
        let config = PostgresArgs::default().into_config(&mut args, &FauxEnv::default());
        assert_eq!(
            config,
            OptOneMany::One(PostgresConfig {
                connection_string: Some("postgres://localhost:5432".to_owned()),
                ..Default::default()
            })
        );
        args.check().unwrap();
    }

    #[test]
    fn merge_into_config2() {
        let mut args = Arguments::new(vec![]);
        let env = FauxEnv(
            vec![
                ("DATABASE_URL", OsString::from("postgres://localhost:5432")),
                ("DEFAULT_SRID", OsString::from("10")),
                ("PGSSLROOTCERT", OsString::from("file")),
            ]
            .into_iter()
            .collect(),
        );
        let config = PostgresArgs::default().into_config(&mut args, &env);
        assert_eq!(
            config,
            OptOneMany::One(PostgresConfig {
                connection_string: Some("postgres://localhost:5432".to_owned()),
                default_srid: Some(10),
                ssl_certificates: PostgresSslCerts {
                    ssl_root_cert: Some(PathBuf::from("file")),
                    ..Default::default()
                },
                ..Default::default()
            })
        );
        args.check().unwrap();
    }

    #[test]
    fn merge_into_config3() {
        let mut args = Arguments::new(vec![]);
        let env = FauxEnv(
            vec![
                ("DATABASE_URL", OsString::from("postgres://localhost:5432")),
                ("DEFAULT_SRID", OsString::from("10")),
                ("PGSSLCERT", OsString::from("cert")),
                ("PGSSLKEY", OsString::from("key")),
                ("PGSSLROOTCERT", OsString::from("root")),
            ]
            .into_iter()
            .collect(),
        );
        let pg_args = PostgresArgs {
            default_srid: Some(20),
            ..Default::default()
        };
        let config = pg_args.into_config(&mut args, &env);
        assert_eq!(
            config,
            OptOneMany::One(PostgresConfig {
                connection_string: Some("postgres://localhost:5432".to_owned()),
                default_srid: Some(20),
                ssl_certificates: PostgresSslCerts {
                    ssl_cert: Some(PathBuf::from("cert")),
                    ssl_key: Some(PathBuf::from("key")),
                    ssl_root_cert: Some(PathBuf::from("root")),
                    unrecognized: UnrecognizedValues::default()
                },
                ..Default::default()
            })
        );
        args.check().unwrap();
    }

    #[test]
    fn cli_ssl_cert_and_key_override_env() {
        let mut args = Arguments::new(vec![]);
        let env = FauxEnv(
            vec![
                ("DATABASE_URL", OsString::from("postgres://localhost:5432")),
                ("PGSSLCERT", OsString::from("env-cert")),
                ("PGSSLKEY", OsString::from("env-key")),
            ]
            .into_iter()
            .collect(),
        );
        let pg_args = PostgresArgs {
            ssl_cert: Some(PathBuf::from("cli-cert")),
            ssl_key: Some(PathBuf::from("cli-key")),
            ..Default::default()
        };
        let config = pg_args.into_config(&mut args, &env);
        assert_eq!(
            config,
            OptOneMany::One(PostgresConfig {
                connection_string: Some("postgres://localhost:5432".to_owned()),
                ssl_certificates: PostgresSslCerts {
                    ssl_cert: Some(PathBuf::from("cli-cert")),
                    ssl_key: Some(PathBuf::from("cli-key")),
                    ..Default::default()
                },
                ..Default::default()
            })
        );
        args.check().unwrap();
    }

    #[test]
    fn override_config_applies_ssl_cli_flags() {
        let mut config = OptOneMany::One(PostgresConfig {
            connection_string: Some("postgres://localhost:5432".to_owned()),
            ssl_certificates: PostgresSslCerts {
                ssl_cert: Some(PathBuf::from("from-config")),
                ..Default::default()
            },
            ..Default::default()
        });
        PostgresArgs {
            ssl_cert: Some(PathBuf::from("from-cli")),
            ssl_key: Some(PathBuf::from("key-from-cli")),
            ..Default::default()
        }
        .override_config(&mut config);
        let OptOneMany::One(cfg) = config else {
            panic!("expected exactly one postgres config");
        };
        assert_eq!(
            cfg.ssl_certificates.ssl_cert,
            Some(PathBuf::from("from-cli"))
        );
        assert_eq!(
            cfg.ssl_certificates.ssl_key,
            Some(PathBuf::from("key-from-cli"))
        );
    }
}
