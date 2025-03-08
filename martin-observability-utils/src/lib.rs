#![doc = include_str!("../README.md")]

#[derive(Default)]
pub struct MartinObservability {
    log_format: LogFormat,
    log_level: LogLevel,
}
impl MartinObservability {
    /// Set the log format for the application
    #[must_use]
    pub fn with_log_format(mut self, log_format: LogFormat) -> Self {
        self.log_format = log_format;
        self
    }
    /// Set the log level for the application
    #[must_use]
    pub fn with_log_level(mut self, log_level: LogLevel) -> Self {
        self.log_level = log_level;
        self
    }
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
        let registry = tracing_subscriber::registry()
            .with(tracing_subscriber::filter::EnvFilter::from(self.log_level));
        match self.log_format {
            LogFormat::Full => set_global_default(registry.with(Layer::default())),
            LogFormat::Compact => set_global_default(registry.with(Layer::default().json())),
            LogFormat::Pretty => set_global_default(registry.with(Layer::default().pretty())),
            LogFormat::Json => set_global_default(registry.with(Layer::default().compact())),
        }
        .expect("since martin has not set the global_default, no global default is set");
    }
}

#[derive(PartialEq, Eq, Clone, Copy, Default)]
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

pub struct LogLevel {
    key: String,
    level: tracing::Level,
}
impl LogLevel {
    /// Get log level from an environment variable
    ///
    /// Default: [`LogLevel::default`]
    #[must_use]
    pub fn from_env_var(key: &'static str) -> Self {
        let mut level = Self::default();
        if std::env::var(key).is_ok() {
            level.key = key.to_string();
        }
        level
    }
    /// Sets the environment variable key for the log level if it exsts
    #[must_use]
    pub fn or_default(mut self, level: tracing::Level) -> Self {
        self.level = level;
        self
    }
}
impl Default for LogLevel {
    fn default() -> Self {
        LogLevel {
            key: "RUST_LOG".to_string(),
            level: tracing::Level::INFO,
        }
    }
}

impl From<LogLevel> for tracing_subscriber::filter::EnvFilter {
    fn from(value: LogLevel) -> Self {
        tracing_subscriber::filter::EnvFilter::builder()
            .with_env_var(value.key)
            .with_default_directive(value.level.into())
            .from_env_lossy()
    }
}
