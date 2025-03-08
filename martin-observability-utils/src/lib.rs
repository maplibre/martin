#[derive(Default)]
pub struct MartinObservability {
    log_format: LogFormat,
    log_level: LogLevel,
}
impl MartinObservability {
    /// Set the log format for the application
    pub fn with_log_format(mut self, log_format: LogFormat) -> Self {
        self.log_format = log_format;
        self
    }
    /// Set the log level for the application
    pub fn with_log_level(mut self, log_level: LogLevel) -> Self {
        self.log_level = log_level;
        self
    }
    /// transform [`log`](https://docs.rs/log) records into [`tracing`](https://docs.rs/tracing) [`Event`](tracing::Event)s.
    ///
    /// # Errors
    /// This function will panic if the global `log`-logger cannot be set.
    /// This only happens if the global `log`-logger has already been set.
    pub fn with_initialised_log_tracing(self) -> Self {
        tracing_log::LogTracer::builder()
            .with_interest_cache(tracing_log::InterestCacheConfig::default())
            .init()
            .expect("the global logger to only be set once");
        self
    }
    /// Set the global subscriber for the application.
    ///
    /// # Errors
    /// This function will panic if the global subscriber cannot be set.
    /// This only happens if the global subscriber has already been set.
    pub fn set_global_subscriber(self) {
        use tracing_subscriber::fmt::Layer;
        use tracing_subscriber::prelude::*;
        let registry = tracing_subscriber::registry()
            .with(tracing_subscriber::filter::EnvFilter::from(self.log_level))
            .with((self.log_format == LogFormat::Json).then(|| Layer::default().json()))
            .with((self.log_format == LogFormat::Pretty).then(|| Layer::default().pretty()))
            .with((self.log_format == LogFormat::Compact).then(|| Layer::default().compact()));
        tracing::subscriber::set_global_default(registry)
            .expect("since martin has not set the global_default, no global default is set");
    }
}

#[derive(PartialEq, Eq, Clone, Copy, Default)]
pub enum LogFormat {
    Json,
    Pretty,
    #[default]
    Compact,
}
impl LogFormat {
    pub fn from_env_var(key: &'static str) -> Self {
        match std::env::var(key).unwrap_or_default().as_str() {
            "json" | "jsonl" => LogFormat::Json,
            "pretty" | "verbose" => LogFormat::Pretty,
            _ => LogFormat::Compact,
        }
    }
}

pub struct LogLevel {
    key: String,
    level: tracing::Level,
}
impl LogLevel {
    /// environment variable key for the log level
    pub fn from_env_var(key: &'static str) -> Self {
        let mut level = Self::default();
        if std::env::var(key).is_ok() {
            level.key = key.to_string();
        }
        level
    }
    /// Sets the environment variable key for the log level if it exsts
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
