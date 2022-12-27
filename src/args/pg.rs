use log::{info, warn};

use crate::args::environment::Env;
use crate::args::root::MetaArgs;
use crate::pg::{PgConfig, POOL_SIZE_DEFAULT};
use crate::utils::OneOrMany;

#[derive(clap::Args, Debug, PartialEq, Default)]
#[command(about, version)]
pub struct PgArgs {
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
    pub fn into_config(self, meta: &mut MetaArgs, env: &impl Env) -> Option<OneOrMany<PgConfig>> {
        let connections = Self::extract_conn_strings(meta, env);
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
                ..Default::default()
            })
            .collect();

        match results.len() {
            0 => None,
            1 => Some(OneOrMany::One(results.into_iter().next().unwrap())),
            _ => Some(OneOrMany::Many(results)),
        }
    }

    pub fn override_config(self, pg_config: &mut OneOrMany<PgConfig>, env: &impl Env) {
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

    fn extract_conn_strings(meta: &mut MetaArgs, env: &impl Env) -> Vec<String> {
        let mut strings = Vec::new();
        let mut i = 0;
        while i < meta.connection.len() {
            if is_postgresql_string(&meta.connection[i]) {
                strings.push(meta.connection.remove(i));
            } else {
                i += 1;
            }
        }
        if strings.is_empty() {
            if let Some(s) = env.get_env_str("DATABASE_URL") {
                if is_postgresql_string(&s) {
                    info!("Using env var DATABASE_URL to connect to PostgreSQL");
                    strings.push(s);
                } else {
                    warn!("Environment var DATABASE_URL is not a valid postgres connection string");
                }
            }
        }
        strings
    }

    fn get_default_srid(&self, env: &impl Env) -> Option<i32> {
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
    fn get_accept_invalid_cert(&self, env: &impl Env) -> bool {
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
    fn get_ca_root_file(&self, env: &impl Env) -> Option<std::path::PathBuf> {
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

    #[test]
    fn test_extract_conn_strings() {
        let mut meta = MetaArgs {
            connection: vec![
                "postgresql://localhost:5432".to_string(),
                "postgres://localhost:5432".to_string(),
                "mysql://localhost:3306".to_string(),
            ],
            ..Default::default()
        };
        assert_eq!(
            PgArgs::extract_conn_strings(&mut meta, &FauxEnv::default()),
            vec!["postgresql://localhost:5432", "postgres://localhost:5432"]
        );
        assert_eq!(meta.connection, vec!["mysql://localhost:3306"]);
    }

    #[test]
    fn test_extract_conn_strings_from_env() {
        let mut meta = MetaArgs {
            ..Default::default()
        };
        let env = FauxEnv(
            vec![("DATABASE_URL", os("postgresql://localhost:5432"))]
                .into_iter()
                .collect(),
        );
        let strings = PgArgs::extract_conn_strings(&mut meta, &env);
        assert_eq!(strings, vec!["postgresql://localhost:5432"]);
        assert_eq!(meta.connection, Vec::<String>::new());
    }

    #[test]
    fn test_merge_into_config() {
        let mut meta = MetaArgs {
            connection: vec!["postgres://localhost:5432".to_string()],
            ..Default::default()
        };
        let config = PgArgs::default().into_config(&mut meta, &FauxEnv::default());
        assert_eq!(
            config,
            Some(OneOrMany::One(PgConfig {
                connection_string: some("postgres://localhost:5432"),
                ..Default::default()
            }))
        );
        assert_eq!(meta.connection, Vec::<String>::new());
    }

    #[test]
    fn test_merge_into_config2() {
        let mut meta = MetaArgs::default();
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
        let config = PgArgs::default().into_config(&mut meta, &env);
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
    }

    #[test]
    fn test_merge_into_config3() {
        let mut meta = MetaArgs::default();
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
        let config = pg_args.into_config(&mut meta, &env);
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
    }
}
