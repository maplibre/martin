use std::time::Duration;

use clap::ValueEnum;
use log::{info, warn};
use serde::{Deserialize, Serialize};

use crate::args::connections::Arguments;
use crate::args::connections::State::{Ignore, Take};
use crate::args::environment::Env;
use crate::pg::{PgConfig, PgSslCerts, POOL_SIZE_DEFAULT};
use crate::utils::{OptBoolObj, OptOneMany};

// Must match the help string for BoundsType::Quick
pub const DEFAULT_BOUNDS_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(PartialEq, Eq, Default, Debug, Clone, Copy, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum BoundsCalcType {
    /// Compute table geometry bounds, but abort if it takes longer than 5 seconds.
    #[default]
    Quick,
    /// Compute table geometry bounds. The startup time may be significant. Make sure all GEO columns have indexes.
    Calc,
    /// Skip bounds calculation. The bounds will be set to the whole world.
    Skip,
}

#[derive(clap::Args, Debug, PartialEq, Default)]
#[command(about, version)]
pub struct PgArgs {
    /// Specify how bounds should be computed for the spatial PG tables. [DEFAULT: quick]
    #[arg(short = 'b', long)]
    pub auto_bounds: Option<BoundsCalcType>,
    /// Loads trusted root certificates from a file. The file should contain a sequence of PEM-formatted CA certificates.
    #[arg(long)]
    pub ca_root_file: Option<std::path::PathBuf>,
    /// If a spatial PG table has SRID 0, then this default SRID will be used as a fallback.
    #[arg(short, long)]
    pub default_srid: Option<i32>,
    #[arg(help = format!("Maximum Postgres connections pool size [DEFAULT: {}]", POOL_SIZE_DEFAULT), short, long)]
    pub pool_size: Option<usize>,
    /// Limit the number of features in a tile from a PG table source.
    #[arg(short, long)]
    pub max_feature_count: Option<usize>,
}

impl PgArgs {
    pub fn into_config<'a>(
        self,
        cli_strings: &mut Arguments,
        env: &impl Env<'a>,
    ) -> OptOneMany<PgConfig> {
        let connections = Self::extract_conn_strings(cli_strings, env);
        let default_srid = self.get_default_srid(env);
        let certs = self.get_certs(env);

        let results: Vec<_> = connections
            .into_iter()
            .map(|s| PgConfig {
                connection_string: Some(s),
                ssl_certificates: certs.clone(),
                default_srid,
                auto_bounds: self.auto_bounds,
                max_feature_count: self.max_feature_count,
                pool_size: self.pool_size,
                auto_publish: OptBoolObj::NoValue,
                tables: None,
                functions: None,
            })
            .collect();

        match results.len() {
            0 => OptOneMany::NoVals,
            1 => OptOneMany::One(results.into_iter().next().unwrap()),
            _ => OptOneMany::Many(results),
        }
    }

    pub fn override_config<'a>(self, pg_config: &mut OptOneMany<PgConfig>, env: &impl Env<'a>) {
        if self.default_srid.is_some() {
            info!("Overriding configured default SRID to {} on all Postgres connections because of a CLI parameter", self.default_srid.unwrap());
            pg_config.iter_mut().for_each(|c| {
                c.default_srid = self.default_srid;
            });
        }
        if self.pool_size.is_some() {
            info!("Overriding configured pool size to {} on all Postgres connections because of a CLI parameter", self.pool_size.unwrap());
            pg_config.iter_mut().for_each(|c| {
                c.pool_size = self.pool_size;
            });
        }
        if self.max_feature_count.is_some() {
            info!("Overriding maximum feature count to {} on all Postgres connections because of a CLI parameter", self.max_feature_count.unwrap());
            pg_config.iter_mut().for_each(|c| {
                c.max_feature_count = self.max_feature_count;
            });
        }

        if self.ca_root_file.is_some() {
            info!("Overriding root certificate file to {} on all Postgres connections because of a CLI parameter",
                self.ca_root_file.as_ref().unwrap().display());
            pg_config.iter_mut().for_each(|c| {
                c.ssl_certificates.ssl_root_cert = self.ca_root_file.clone();
            });
        }

        for v in &[
            "CA_ROOT_FILE",
            "DANGER_ACCEPT_INVALID_CERTS",
            "DATABASE_URL_PAT",
            "DEFAULT_SRID",
            "PGSSLCERT",
            "PGSSLKEY",
            "PGSSLROOTCERT",
        ] {
            // We don't want to warn about these in case they were used in the config file expansion
            if env.has_unused_var(v) {
                warn!("Environment variable {v} is set, but will be ignored because a configuration file was loaded. Any environment variables can be used inside the config yaml file.");
            }
        }
    }

    fn extract_conn_strings<'a>(cli_strings: &mut Arguments, env: &impl Env<'a>) -> Vec<String> {
        let mut connections = cli_strings.process(|v| {
            if is_postgresql_string(v) {
                Take(v.to_string())
            } else {
                Ignore
            }
        });
        if connections.is_empty() {
            if let Some(s) = env.get_env_str("DATABASE_URL_PAT") {
                if is_postgresql_string(&s) {
                    info!("Using env var DATABASE_URL_PAT to connect to PostgreSQL");
                    connections.push(s);
                } else {
                    warn!("Environment var DATABASE_URL_PAT is not a valid postgres connection string");
                }
            }
        }
        connections
    }

    fn get_default_srid<'a>(&self, env: &impl Env<'a>) -> Option<i32> {
        if self.default_srid.is_some() {
            return self.default_srid;
        }
        env.get_env_str("DEFAULT_SRID")
            .and_then(|srid| match srid.parse::<i32>() {
                Ok(v) => {
                    info!("Using env var DEFAULT_SRID={v} to set default SRID");
                    Some(v)
                }
                Err(v) => {
                    warn!("Env var DEFAULT_SRID is not a valid integer {srid}: {v}");
                    None
                }
            })
    }

    fn get_certs<'a>(&self, env: &impl Env<'a>) -> PgSslCerts {
        let mut result = PgSslCerts {
            ssl_cert: Self::parse_env_var(env, "PGSSLCERT", "ssl certificate"),
            ssl_key: Self::parse_env_var(env, "PGSSLKEY", "ssl key for certificate"),
            ssl_root_cert: self.ca_root_file.clone(),
        };
        if result.ssl_root_cert.is_none() {
            result.ssl_root_cert = Self::parse_env_var(env, "PGSSLROOTCERT", "root certificate(s)");
        }
        if result.ssl_root_cert.is_none() {
            result.ssl_root_cert = Self::parse_env_var(
                env,
                "CA_ROOT_FILE",
                "root certificate(s). This setting is obsolete, please use PGSSLROOTCERT instead",
            );
        }

        result
    }

    fn parse_env_var<'a>(
        env: &impl Env<'a>,
        env_var: &str,
        info: &str,
    ) -> Option<std::path::PathBuf> {
        let path = env.var_os(env_var).map(std::path::PathBuf::from);
        if let Some(p) = &path {
            let p = p.display();
            info!("Using env {env_var}={p} to load {info}");
        }
        path
    }
}

#[must_use]
fn is_postgresql_string(s: &str) -> bool {
    s.starts_with("postgresql://") || s.starts_with("postgres://")
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::test_utils::{os, some, FauxEnv};
    use crate::MartinError;

    #[test]
    fn test_extract_conn_strings() {
        let mut args = Arguments::new(vec![
            "postgresql://localhost:5432".to_string(),
            "postgres://localhost:5432".to_string(),
            "mysql://localhost:3306".to_string(),
        ]);
        assert_eq!(
            PgArgs::extract_conn_strings(&mut args, &FauxEnv::default()),
            vec!["postgresql://localhost:5432", "postgres://localhost:5432"]
        );
        assert!(matches!(args.check(), Err(
            MartinError::UnrecognizableConnections(v)) if v == vec!["mysql://localhost:3306"]));
    }

    #[test]
    fn test_extract_conn_strings_from_env() {
        let mut args = Arguments::new(vec![]);
        let env = FauxEnv(
            vec![("DATABASE_URL_PAT", os("postgresql://localhost:5432"))]
                .into_iter()
                .collect(),
        );
        let strings = PgArgs::extract_conn_strings(&mut args, &env);
        assert_eq!(strings, vec!["postgresql://localhost:5432"]);
        assert!(args.check().is_ok());
    }

    #[test]
    fn test_merge_into_config() {
        let mut args = Arguments::new(vec!["postgres://localhost:5432".to_string()]);
        let config = PgArgs::default().into_config(&mut args, &FauxEnv::default());
        assert_eq!(
            config,
            OptOneMany::One(PgConfig {
                connection_string: some("postgres://localhost:5432"),
                ..Default::default()
            })
        );
        assert!(args.check().is_ok());
    }

    #[test]
    fn test_merge_into_config2() {
        let mut args = Arguments::new(vec![]);
        let env = FauxEnv(
            vec![
                ("DATABASE_URL_PAT", os("postgres://localhost:5432")),
                ("DEFAULT_SRID", os("10")),
                ("DANGER_ACCEPT_INVALID_CERTS", os("1")),
                ("CA_ROOT_FILE", os("file")),
            ]
            .into_iter()
            .collect(),
        );
        let config = PgArgs::default().into_config(&mut args, &env);
        assert_eq!(
            config,
            OptOneMany::One(PgConfig {
                connection_string: some("postgres://localhost:5432"),
                default_srid: Some(10),
                ssl_certificates: PgSslCerts {
                    ssl_root_cert: Some(PathBuf::from("file")),
                    ..Default::default()
                },
                ..Default::default()
            })
        );
        assert!(args.check().is_ok());
    }

    #[test]
    fn test_merge_into_config3() {
        let mut args = Arguments::new(vec![]);
        let env = FauxEnv(
            vec![
                ("DATABASE_URL_PAT", os("postgres://localhost:5432")),
                ("DEFAULT_SRID", os("10")),
                ("PGSSLCERT", os("cert")),
                ("PGSSLKEY", os("key")),
                ("PGSSLROOTCERT", os("root")),
            ]
            .into_iter()
            .collect(),
        );
        let pg_args = PgArgs {
            default_srid: Some(20),
            ..Default::default()
        };
        let config = pg_args.into_config(&mut args, &env);
        assert_eq!(
            config,
            OptOneMany::One(PgConfig {
                connection_string: some("postgres://localhost:5432"),
                default_srid: Some(20),
                ssl_certificates: PgSslCerts {
                    ssl_cert: Some(PathBuf::from("cert")),
                    ssl_key: Some(PathBuf::from("key")),
                    ssl_root_cert: Some(PathBuf::from("root")),
                },
                ..Default::default()
            })
        );
        assert!(args.check().is_ok());
    }
}
