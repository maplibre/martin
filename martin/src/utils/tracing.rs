use std::str::FromStr;

use tracing_subscriber::fmt::Layer as FormatLayer;
use tracing_subscriber::reload::{Handle, Layer as ReloadLayer};
use tracing_subscriber::{EnvFilter, Layer, Registry};

#[derive(
    PartialEq,
    Eq,
    Clone,
    Copy,
    Default,
    Debug,
    clap::ValueEnum,
    serde::Serialize,
    serde::Deserialize,
)]
pub enum LogFormatOptions {
    /// Emit human-readable, single-line logs.
    /// See [here for a sample](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/fmt/format/struct.Full.html#example-output)
    Full,
    /// A variant of the full-format, optimized for short line lengths.
    /// See [here for a sample](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/fmt/format/struct.Compact.html#example-output)
    #[default]
    Compact,
    /// The bare log without timestamps or modules. Just the level and the log
    Bare,
    /// Excessively pretty, multi-line logs for local development/debugging, prioritizing readability over compact storage.
    /// See [here for a sample](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/fmt/format/struct.Pretty.html#example-output)
    #[serde(alias = "verbose")]
    #[value(alias("verbose"))]
    Pretty,
    /// Output newline-delimited (structured) JSON logs, ***not*** optimized for human readability.
    /// See [here for a sample](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/fmt/format/struct.Json.html#example-output)
    #[serde(alias = "jsonl")]
    #[value(alias("jsonl"))]
    Json,
}
impl LogFormatOptions {
    pub fn from_str_opt(var: &str) -> Option<Self> {
        match var {
            "full" => Some(LogFormatOptions::Full),
            "pretty" | "verbose" => Some(LogFormatOptions::Pretty),
            "json" | "jsonl" => Some(LogFormatOptions::Json),
            "compact" => Some(LogFormatOptions::Compact),
            "bare" => Some(LogFormatOptions::Bare),
            _ => None,
        }
    }
}

pub struct ReloadableTracingConfiguration {
    reload_handle: Handle<
        tracing_subscriber::layer::Layered<
            EnvFilter,
            Box<dyn Layer<Registry> + Send + Sync + 'static>,
            Registry,
        >,
        Registry,
    >,
    default_level: &'static str,
}

impl ReloadableTracingConfiguration {
    /// Transform [`log`](https://docs.rs/log) records into [`tracing`](https://docs.rs/tracing) [`Event`](tracing::Event)s.
    ///
    /// # Panics
    /// This function will panic if the global `log`-logger cannot be set.
    /// This only happens if the global `log`-logger has already been set.
    fn initialise_log_tracing() {
        tracing_log::LogTracer::builder()
            .with_interest_cache(tracing_log::InterestCacheConfig::default())
            .init()
            .expect("the global logger to only be set once");
    }

    /// Initialise the global tracing registry.
    ///
    /// # Panics
    /// This function will panic if the global `log`-logger cannot be set or if the global `tracing`-registry cannot be set.
    #[must_use]
    pub fn init_global_registry(default_level: &'static str) -> Self {
        Self::initialise_log_tracing();
        use tracing_subscriber::prelude::*;
        let default_fmt = FormatLayer::default().boxed();
        let default_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
            EnvFilter::from_str(default_level).expect("the default level should not be invalid")
        });
        // counterintuitive: this is env first, then format. Not a bug!
        let default_layer = default_fmt.and_then(default_filter);
        let (reload_layer, reload_handle) = ReloadLayer::new(default_layer);
        let registry = Registry::default().with(reload_layer);
        tracing::subscriber::set_global_default(registry)
            .expect("since martin has not set the global_default, no global default is set");
        Self {
            reload_handle,
            default_level,
        }
    }
    /// Reload the configured format and level.
    pub fn reload_fmt(&self, format: LogFormatOptions) {
        let log_format_layer = match format {
            LogFormatOptions::Full => FormatLayer::default().boxed(),
            LogFormatOptions::Pretty => FormatLayer::default().pretty().boxed(),
            LogFormatOptions::Json => FormatLayer::default().json().boxed(),
            LogFormatOptions::Compact => FormatLayer::default().compact().boxed(),
            LogFormatOptions::Bare => FormatLayer::default().compact().without_time().boxed(),
        };
        let default_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
            EnvFilter::from_str(self.default_level)
                .expect("the default level should not be invalid")
        });

        // counterintuitive: this is env first, then format. Not a bug!
        self.reload_handle
            .reload(log_format_layer.and_then(default_filter))
            .expect("the subscriber should still exist")
    }
}
