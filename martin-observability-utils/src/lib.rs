#![doc = include_str!("../README.md")]

use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use tracing_subscriber::EnvFilter;

#[derive(Default)]
pub struct MartinObservability {
    log_format: LogFormat,
    filter: EnvFilter,
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
            LogFormat::Full => set_global_default(registry.with(Layer::default())),
            LogFormat::Compact => set_global_default(registry.with(Layer::default().json())),
            LogFormat::Pretty => set_global_default(registry.with(Layer::default().pretty())),
            LogFormat::Json => set_global_default(registry.with(Layer::default().compact())),
        }
        .expect("since martin has not set the global_default, no global default is set");
    }
}
impl From<(EnvFilter,LogFormat)> for MartinObservability {
    fn from((filter, log_format): (EnvFilter, LogFormat)) -> Self {
        Self{log_format,filter}
    }
}

#[derive(PartialEq, Eq, Clone, Copy, Default, Debug, clap::ValueEnum)]
pub enum LogFormat {
    /// Emits human-readable, single-line logs for each event that occurs, with the current span context displayed before the formatted representation of the event.
    /// See [here](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/fmt/format/struct.Full.html#example-output) for sample output.
    Full,
    /// A variant of [`LogFormat::Full`], optimized for short line lengths.
    /// Fields from the current span context are appended to the fields of the formatted event.
    /// See [here](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/fmt/format/struct.Compact.html#example-output) for sample output.
    #[default]
    Compact,
    /// Emits excessively pretty, multi-line logs, optimized for human readability.
    /// This is primarily intended to be used in local development and debugging, or for command-line applications, where automated analysis and compact storage of logs is less of a priority than readability and visual appeal.
    /// See [here](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/fmt/format/struct.Pretty.html#example-output) for sample output.
    Pretty,
    /// Outputs newline-delimited JSON logs.
    /// This is intended for production use with systems where structured logs are consumed as JSON by analysis and viewing tools.
    /// The JSON output is not optimized for human readability.
    /// See [here](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/fmt/format/struct.Json.html#example-output) for sample output.
    Json,
}
impl LogFormat {
    /// log format (how the logs are formatted on the cli) from an environment variable
    ///
    /// Default: [`LogFormat::Compact`]
    #[must_use]
    pub fn from_env_var(key: &'static str) -> Self {
        match std::env::var(key).unwrap_or_default().as_str() {
            "full" => LogFormat::Full,
            "pretty" | "verbose" => LogFormat::Pretty,
            "json" | "jsonl" => LogFormat::Json,
            _ => LogFormat::Compact,
        }
    }
}

/// Allows configuring log directives
///
/// See <https://docs.rs/tracing-subscriber/latest/tracing_subscriber/filter/struct.EnvFilter.html#example-syntax> for more information.
#[derive(Clone, PartialEq, Debug)]
pub struct LogLevel(Option<String>);
impl LogLevel {
    /// Get log directives from an environment variable
    #[must_use]
    pub fn from_env_var(key: &str) -> Self {
        Self(std::env::var(key).ok())
    }
    /// Search for the log level at a path in the CLI
    ///
    /// Due to [`clap`] having a help function, it is not possible to use it.
    /// All errors during this operation are ignored as the default ([`tracing::Level::INFO`]) will print errors for this too during the regular parsing.
    #[must_use]
    pub fn or_from_argument(mut self, argument: &str) -> Self {
        if self.0.is_none() {
            if let Some(arg) = Self::get_next_after_argument(argument) {
                self.0 = Some(arg);
            }
        }
        self
    }
/// Search for the log level at a path in a config file
///
/// All errors during this operation are ignored as the default ([`tracing::Level::INFO`]) will print errors for this too during the regular parsing.
    #[must_use]
    pub fn or_in_config_file(mut self, argument: &str, key: &str) -> Self {
        if self.0.is_none() {
            if let Some(path) = Self::get_next_after_argument(argument) {
                let path = PathBuf::from(path);
                self.0 = Self::read_path_in_file(path.as_path(), key);
            }
        }
        self
    }
    /// Parse a [`EnvFilter`] from the directives in the string to this point, ignoring any that are invalid.
    ///
    /// See <https://docs.rs/tracing-subscriber/latest/tracing_subscriber/filter/struct.EnvFilter.html#example-syntax> for more information.
    #[must_use]
    pub fn lossy_parse_to_filter_with_default(self, default_directives: &str) -> EnvFilter {
        let directives = match self.0 {
            Some(directives) => directives,
            None => default_directives.to_string(),
        };
        EnvFilter::builder().parse_lossy(directives)
    }

    /// Search for the argument following a certain argument in the cli
    #[must_use]
    fn get_next_after_argument(argument: &str) -> Option<String> {
        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            if arg == argument {
                return args.next();
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
}
