#![doc = include_str!("../README.md")]

use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use tracing_subscriber::EnvFilter;

#[derive(Default)]
pub struct MartinObservability {
    filter: EnvFilter,
    log_format: LogFormatOptions,
}
impl MartinObservability {
    /// transform [`log`](https://docs.rs/log) records into [`tracing`](https://docs.rs/tracing) [`Event`](tracing::Event)s.
    ///
    /// # Panics
    /// This function will panic if the global `log`-logger cannot be set.
    /// This only happens if the global `log`-logger has already been set.
    #[must_use]
    pub fn with_initialised_log_tracing(self) -> Self {
        tracing_log::LogTracer::builder()
            .with_interest_cache(tracing_log::InterestCacheConfig::default())
            .init()
            .expect("the global logger to only be set once");
        self
    }
    /// Set the global subscriber for the application.
    ///
    /// # Panics
    /// This function will panic if the global subscriber cannot be set.
    /// This only happens if the global subscriber has already been set.
    pub fn set_global_subscriber(self) {
        use tracing::subscriber::set_global_default;
        use tracing_subscriber::fmt::Layer;
        use tracing_subscriber::prelude::*;
        let registry = tracing_subscriber::registry().with(self.filter);
        match self.log_format {
            LogFormatOptions::Full => set_global_default(registry.with(Layer::default())),
            LogFormatOptions::Compact => set_global_default(registry.with(Layer::default().json())),
            LogFormatOptions::Pretty => {
                set_global_default(registry.with(Layer::default().pretty()))
            }
            LogFormatOptions::Json => set_global_default(registry.with(Layer::default().compact())),
        }
        .expect("since martin has not set the global_default, no global default is set");
    }
}
impl From<(EnvFilter, LogFormatOptions)> for MartinObservability {
    fn from((filter, log_format): (EnvFilter, LogFormatOptions)) -> Self {
        Self { filter, log_format }
    }
}

#[derive(PartialEq, Eq, Clone, Copy, Default, Debug, clap::ValueEnum)]
pub enum LogFormatOptions {
    /// Emit human-readable, single-line logs.
    /// See [here for a sample](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/fmt/format/struct.Full.html#example-output)
    Full,
    /// A variant of the full-format, optimized for short line lengths.
    /// See [here for a sample](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/fmt/format/struct.Compact.html#example-output)
    #[default]
    Compact,
    /// Excessively pretty, multi-line logs for local development/debugging, prioritizing readability over compact storage.
    /// See [here for a sample](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/fmt/format/struct.Pretty.html#example-output)
    Pretty,
    /// Output newline-delimited (structured) JSON logs, ***not*** optimized for human readability.
    /// See [here for a sample](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/fmt/format/struct.Json.html#example-output)
    Json,
}
impl LogFormatOptions {
    fn from_str(key: &str) -> Option<Self> {
        match std::env::var(key).unwrap_or_default().as_str() {
            "full" => Some(LogFormatOptions::Full),
            "pretty" | "verbose" => Some(LogFormatOptions::Pretty),
            "json" | "jsonl" => Some(LogFormatOptions::Json),
            "compact" => Some(LogFormatOptions::Compact),
            _ => None,
        }
    }
}

pub struct LogFormat(Option<LogFormatOptions>);
impl LogFormat {
    /// Search for the log format (how the logs are formatted on the cli) as an argument in the CLI
    ///
    /// Due to [`clap`] having a help function, it is not possible to use it.
    #[must_use]
    pub fn from_argument(argument: &str) -> Self {
        let args = std::env::args().collect::<Vec<String>>();
        let v = get_next_after_argument(argument, &args);
        if let Some(v) = v {
            if let Some(v) = LogFormatOptions::from_str(&v) {
                Self(Some(v))
            } else {
                eprintln!("Ignoring specified cli argument {argument} {v} as it is not a valid log format. Can be one of full, compact, pretty, json");
                Self(None)
            }
        } else {
            Self(None)
        }
    }
    /// Search for the log format (how the logs are formatted on the cli) at a path in a config file
    #[must_use]
    pub fn or_in_config_file(mut self, argument: &str, key: &str) -> Self {
        if self.0.is_none() {
            let args = std::env::args().collect::<Vec<String>>();
            if let Some(path) = get_next_after_argument(argument, &args) {
                let path = PathBuf::from(path);
                if let Some(v) = read_path_in_file(&path, key) {
                    match LogFormatOptions::from_str(&v) {
                        Some(v) => self.0=Some(v),
                        None => eprintln!("Ignoring specified option {key}: {v} inside {path:?} as it is not a valid log format. Can be one of full, compact, pretty, json"),
                    }
                }
            }
        }
        self
    }
    /// Gets log format (how the logs are formatted on the cli) from an environment variable
    #[must_use]
    pub fn or_env_var(mut self, key: &'static str) -> Self {
        if self.0.is_none() {
            if let Ok(v) = std::env::var(key) {
                match LogFormatOptions::from_str(&v) {
                    Some(v) => self.0=Some(v),
                    None => eprintln!("Ignoring specified environment variable {key}={v} as it is not a valid log format. Can be one of full, compact, pretty, json"),
                }
            }
        }
        self
    }
    /// Sets a default
    #[must_use]
    pub fn or_default(self, default_format: LogFormatOptions) -> LogFormatOptions {
        self.0.unwrap_or(default_format)
    }
}

/// Allows configuring log directives
///
/// See [here](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/filter/struct.EnvFilter.html#example-syntax) for more information.
#[derive(Clone, PartialEq, Debug)]
pub struct LogLevel(Option<String>);
impl LogLevel {
    /// Search for the log level at a path in the CLI
    ///
    /// Due to [`clap`] having a help function, it is not possible to use it.
    /// All errors during this operation are ignored as the default ([`tracing::Level::INFO`]) will print errors for this too during the regular parsing.
    #[must_use]
    pub fn from_argument(argument: &str) -> Self {
        let args = std::env::args().collect::<Vec<String>>();
        Self(get_next_after_argument(argument, &args))
    }
    /// Search for the log level at a path in a config file
    ///
    /// All errors during this operation are ignored as the default ([`tracing::Level::INFO`]) will print errors for this too during the regular parsing.
    #[must_use]
    pub fn or_in_config_file(mut self, argument: &str, key: &str) -> Self {
        if self.0.is_none() {
            let args = std::env::args().collect::<Vec<String>>();
            if let Some(path) = get_next_after_argument(argument, &args) {
                let path = PathBuf::from(path);
                self.0 = read_path_in_file(path.as_path(), key);
            }
        }
        self
    }
    /// Get log directives from an environment variable
    #[must_use]
    pub fn or_env_var(mut self, key: &str) -> Self {
        if self.0.is_none() {
            self.0 = std::env::var(key).ok();
        }
        self
    }
    /// Parse a [`EnvFilter`] from the directives in the string to this point, ignoring any that are invalid.
    ///
    /// See [here](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/filter/struct.EnvFilter.html#example-syntax) for more information.
    #[must_use]
    pub fn lossy_parse_to_filter_with_default(self, default_directives: &str) -> EnvFilter {
        let directives = match self.0 {
            Some(directives) => directives,
            None => default_directives.to_string(),
        };
        EnvFilter::builder().parse_lossy(directives)
    }
}

/// Search for the argument following a certain argument in the cli
#[must_use]
fn get_next_after_argument(argument: &str, args: &[String]) -> Option<String> {
    let mut args = args.iter();
    let _ = args.next(); // first argument is binary
    while let Some(arg) = args.next() {
        if arg == argument {
            return args.next().cloned();
        }
    }
    None
}
/// Reads a key from a yaml file at a path
///
/// All errors are ignored and return [`None`]
#[must_use]
fn read_path_in_file(path: &Path, key: &str) -> Option<String> {
    let mut config_file = Vec::new();
    let _ = File::open(path).ok()?.read_to_end(&mut config_file).ok()?;
    let map: HashMap<String, serde_yaml::Value> = serde_yaml::from_slice(&config_file).ok()?;
    if let Some(v) = map.get(key) {
        if let Some(v) = v.as_str() {
            return Some(v.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_log_level_env_var() {
        std::env::set_var("TEST_LOG_LEVEL_DEBUG", "debug");

        let log_level = LogLevel(None).or_env_var("TEST_NOT_EXISTING_VARIABLE");
        assert_eq!(log_level, LogLevel(None));
        let log_level = LogLevel(None).or_env_var("TEST_LOG_LEVEL_DEBUG");
        assert_eq!(log_level, LogLevel(Some("debug".to_string())));
    }

    #[test]
    fn test_get_next_after_argument() {
        let args = vec![
            "binary-path-goes-here".to_string(),
            "--log-level".to_string(),
            "trace".to_string(),
            "--log-level2".to_string(),
        ];
        let log_level = get_next_after_argument("not-found", &args);
        assert_eq!(log_level, None);
        let log_level = get_next_after_argument("binary-path-goes-here", &args);
        assert_eq!(log_level, None); // should be skipped
        let log_level = get_next_after_argument("--log-level", &args);
        assert_eq!(log_level, Some("trace".to_string()));
        let log_level = get_next_after_argument("--log-level2", &args);
        assert_eq!(log_level, None);
    }

    #[test]
    fn test_read_path_in_file() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.yaml");

        let log_level = read_path_in_file(&config_path, "log_level");
        assert_eq!(log_level, None);

        let mut file = File::create(&config_path).unwrap();
        file.write_all("log_level: warn".as_bytes()).unwrap();

        let log_level = read_path_in_file(&config_path, "key_not_found");
        assert_eq!(log_level, None);
        let log_level = read_path_in_file(&config_path, "log_level");
        assert_eq!(log_level, Some("warn".to_string()));
    }

    #[test]
    fn test_lossy_parse_to_filter_with_default() {
        let log_level = LogLevel(Some("info".to_string()));
        let filter = log_level.lossy_parse_to_filter_with_default("warn");
        assert_eq!(filter.to_string(), "info");
        let filter =
            LogLevel(Some("adsdas".to_string())).lossy_parse_to_filter_with_default("warn");
        assert_eq!(filter.to_string(), "");

        let default_filter = LogLevel(None).lossy_parse_to_filter_with_default("warn");
        assert_eq!(default_filter.to_string(), "warn");
    }
}
