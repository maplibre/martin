use log::{info, warn};

use crate::args::connections::Arguments;
use crate::args::connections::State::{Ignore, Take};
use crate::args::environment::Env;
use crate::pg::{PgConfig, POOL_SIZE_DEFAULT};
use crate::utils::OneOrMany;

#[derive(clap::Args, Debug, PartialEq, Default)]
#[command(about, version)]
pub struct PgArgs {
    /// Disable the automatic generation of bounds for spatial tables.
    #[arg(short = 'b', long)]
    pub disable_bounds: bool,
    /// Loads trusted root certificates from a file. The file should contain a sequence of PEM-formatted CA certificates.
    #[cfg(feature = "ssl")]
    #[arg(long)]
    pub ca_root_file: Option<std::path::PathBuf>,
    /// Trust invalid certificates. This introduces significant vulnerabilities, and should only be used as a last resort.
    #[cfg(feature = "ssl")]
    #[arg(long)]
    pub danger_accept_invalid_certs: bool,
    /// If a spatial table has SRID 0, then this default SRID will be used as a fallback.
    #[arg(short, long)]
    pub default_srid: Option<i32>,
    #[arg(help = format!("Maximum connections pool size [DEFAULT: {}]", POOL_SIZE_DEFAULT), short, long)]
    pub pool_size: Option<u32>,
}

impl PgArgs {
    pub fn into_config<'a>(
        self,
        cli_strings: &mut Arguments,
        env: &impl Env<'a>,
    ) -> Option<OneOrMany<PgConfig>> {
        let connections = Self::extract_conn_strings(cli_strings, env);
        let default_srid = self.get_default_srid(env);
        #[cfg(feature = "ssl")]
        let ca_root_file = self.get_ca_root_file(env);
        #[cfg(feature = "ssl")]
        let danger_accept_invalid_certs = self.get_accept_invalid_cert(env);

        let results: Vec<_> = connections
            .into_iter()
            .map(|s| PgConfig {
                connection_string: Some(s),
                #[cfg(feature = "ssl")]
                ca_root_file: ca_root_file.clone(),
                #[cfg(feature = "ssl")]
                danger_accept_invalid_certs,
                default_srid,
                pool_size: self.pool_size,
                disable_bounds: if self.disable_bounds {
                    Some(true)
                } else {
                    None
                },
                auto_publish: None,
                tables: None,
                functions: None,
            })
            .collect();

        match results.len() {
            0 => None,
            1 => Some(OneOrMany::One(results.into_iter().next().unwrap())),
            _ => Some(OneOrMany::Many(results)),
        }
    }

    pub fn override_config<'a>(self, pg_config: &mut OneOrMany<PgConfig>, env: &impl Env<'a>) {
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
        #[cfg(feature = "ssl")]
        if self.ca_root_file.is_some() {
            info!("Overriding root certificate file to {} on all Postgres connections because of a CLI parameter",
                self.ca_root_file.as_ref().unwrap().display());
            pg_config.iter_mut().for_each(|c| {
                c.ca_root_file = self.ca_root_file.clone();
            });
        }
        #[cfg(feature = "ssl")]
        if self.danger_accept_invalid_certs {
            info!("Overriding configured setting: all Postgres connections will accept invalid certificates because of a CLI parameter. This is a dangerous option, and should not be used if possible.");
            pg_config.iter_mut().for_each(|c| {
                c.danger_accept_invalid_certs = self.danger_accept_invalid_certs;
            });
        }

        for v in &[
            "CA_ROOT_FILE",
            "DANGER_ACCEPT_INVALID_CERTS",
            "DATABASE_URL",
            "DEFAULT_SRID",
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
            if let Some(s) = env.get_env_str("DATABASE_URL") {
                if is_postgresql_string(&s) {
                    info!("Using env var DATABASE_URL to connect to PostgreSQL");
                    connections.push(s);
                } else {
                    warn!("Environment var DATABASE_URL is not a valid postgres connection string");
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

    #[cfg(feature = "ssl")]
    fn get_accept_invalid_cert<'a>(&self, env: &impl Env<'a>) -> bool {
        if !self.danger_accept_invalid_certs
            && env.get_env_str("DANGER_ACCEPT_INVALID_CERTS").is_some()
        {
            info!("Using env var DANGER_ACCEPT_INVALID_CERTS to trust invalid certificates");
            true
        } else {
            self.danger_accept_invalid_certs
        }
    }
    #[cfg(feature = "ssl")]
    fn get_ca_root_file<'a>(&self, env: &impl Env<'a>) -> Option<std::path::PathBuf> {
        if self.ca_root_file.is_some() {
            return self.ca_root_file.clone();
        }
        let path = env.var_os("CA_ROOT_FILE").map(std::path::PathBuf::from);
        if let Some(path) = &path {
            info!(
                "Using env var CA_ROOT_FILE={} to load trusted root certificates",
                path.display()
            );
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
    use super::*;
    use crate::test_utils::{os, some, FauxEnv};
    use crate::Error;

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
            Error::UnrecognizableConnections(v)) if v == vec!["mysql://localhost:3306"]));
    }

    #[test]
    fn test_extract_conn_strings_from_env() {
        let mut args = Arguments::new(vec![]);
        let env = FauxEnv(
            vec![("DATABASE_URL", os("postgresql://localhost:5432"))]
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
            Some(OneOrMany::One(PgConfig {
                connection_string: some("postgres://localhost:5432"),
                ..Default::default()
            }))
        );
        assert!(args.check().is_ok());
    }

    #[test]
    fn test_merge_into_config2() {
        let mut args = Arguments::new(vec![]);
        let env = FauxEnv(
            vec![
                ("DATABASE_URL", os("postgres://localhost:5432")),
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
            Some(OneOrMany::One(PgConfig {
                connection_string: some("postgres://localhost:5432"),
                default_srid: Some(10),
                #[cfg(feature = "ssl")]
                danger_accept_invalid_certs: true,
                #[cfg(feature = "ssl")]
                ca_root_file: Some(std::path::PathBuf::from("file")),
                ..Default::default()
            }))
        );
        assert!(args.check().is_ok());
    }

    #[test]
    fn test_merge_into_config3() {
        let mut args = Arguments::new(vec![]);
        let env = FauxEnv(
            vec![
                ("DATABASE_URL", os("postgres://localhost:5432")),
                ("DEFAULT_SRID", os("10")),
                ("CA_ROOT_FILE", os("file")),
            ]
            .into_iter()
            .collect(),
        );
        let pg_args = PgArgs {
            #[cfg(feature = "ssl")]
            ca_root_file: Some(std::path::PathBuf::from("file2")),
            #[cfg(feature = "ssl")]
            danger_accept_invalid_certs: true,
            default_srid: Some(20),
            ..Default::default()
        };
        let config = pg_args.into_config(&mut args, &env);
        assert_eq!(
            config,
            Some(OneOrMany::One(PgConfig {
                connection_string: some("postgres://localhost:5432"),
                default_srid: Some(20),
                #[cfg(feature = "ssl")]
                danger_accept_invalid_certs: true,
                #[cfg(feature = "ssl")]
                ca_root_file: Some(std::path::PathBuf::from("file2")),
                ..Default::default()
            }))
        );
        assert!(args.check().is_ok());
    }
}
