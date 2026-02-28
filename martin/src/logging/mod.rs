//! Logging initialization for Martin using `tracing` and `tracing-subscriber`.
//!
//! This module provides static logging configuration controlled by:
//! - [`EnvFilter`]: Controls log level filtering (standard tracing-subscriber behavior)
//! - [`LogFormat`]: Controls output format (json, full, compact, bare, pretty)

use std::str::FromStr;

use tracing::Level;
use tracing::level_filters::LevelFilter;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::layer::SubscriberExt as _;

pub mod progress;

/// Log output format options.
#[derive(Debug, Clone, Copy)]
pub enum LogFormat {
    /// Emit human-readable, single-line logs.
    /// See [format::Full](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/fmt/format/struct.Full.html#example-output)
    Full,

    /// A variant of the full-format, optimized for short line lengths (default).
    /// See [format::Compact](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/fmt/format/struct.Compact.html#example-output)
    Compact,

    /// A very bare format, optimized for short line lengths, without timestamps, spans, locations or ANSI colors.
    Bare,

    /// Excessively pretty, multi-line logs for local development/debugging.
    /// See [format::Pretty](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/fmt/format/struct.Pretty.html#example-output)
    Pretty,

    /// Output newline-delimited (structured) JSON logs.
    /// See [format::Json](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/fmt/format/struct.Json.html#example-output)
    Json,
}

impl LogFormat {
    /// Initialize logging according to the selected format.
    pub fn init(self, env_filter: EnvFilter) {
        let dispatch = match self {
            LogFormat::Full => tracing_subscriber::fmt()
                .with_span_events(FmtSpan::NONE)
                .with_env_filter(env_filter)
                .finish()
                .into(),
            LogFormat::Compact => tracing_subscriber::fmt()
                .compact()
                .with_span_events(FmtSpan::NONE)
                .with_env_filter(env_filter)
                .finish()
                .into(),
            LogFormat::Pretty => tracing_subscriber::fmt()
                .pretty()
                .with_env_filter(env_filter)
                .finish()
                .into(),
            LogFormat::Bare => tracing_subscriber::fmt()
                .compact()
                .with_span_events(FmtSpan::NONE)
                .without_time()
                .with_target(false)
                .with_ansi(false)
                .with_env_filter(env_filter)
                .finish()
                .into(),
            LogFormat::Json => tracing_subscriber::fmt()
                .json()
                .with_span_events(FmtSpan::NONE)
                .with_env_filter(env_filter)
                .finish()
                .into(),
        };
        tracing::dispatcher::set_global_default(dispatch)
            .expect("failed to set global default subscriber");
    }
    /// Initialize logging according to the selected format with a progress bar.
    ///
    /// Uses `tracing::dispatcher::set_global_default` directly instead of
    /// `SubscriberInitExt::init()` for the same reason as [`Self::init`]:
    /// to prevent `tracing-subscriber`'s `tracing-log` feature from installing
    /// its own `LogTracer`, which would conflict with `init_log_bridge`.
    pub fn init_with_progress(self, env_filter: EnvFilter) {
        use tracing_subscriber::fmt::layer as fmt_layer;

        let registry = tracing_subscriber::registry().with(env_filter);

        // code below looks duplicated, but it has to be this way due to how types currently work.
        // maybe there is a better way that I can not see
        let dispatch = match self {
            LogFormat::Full => {
                let indicatif_layer = tracing_indicatif::IndicatifLayer::new();
                registry
                    .with(
                        fmt_layer()
                            .with_span_events(FmtSpan::NONE)
                            .with_writer(indicatif_layer.get_stderr_writer()),
                    )
                    .with(indicatif_layer)
                    .into()
            }
            LogFormat::Compact => {
                let indicatif_layer = tracing_indicatif::IndicatifLayer::new();
                registry
                    .with(
                        fmt_layer()
                            .compact()
                            .with_span_events(FmtSpan::NONE)
                            .with_writer(indicatif_layer.get_stderr_writer()),
                    )
                    .with(indicatif_layer)
                    .into()
            }
            LogFormat::Pretty => {
                let indicatif_layer = tracing_indicatif::IndicatifLayer::new();
                registry
                    .with(
                        fmt_layer()
                            .pretty()
                            .with_writer(indicatif_layer.get_stderr_writer()),
                    )
                    .with(indicatif_layer)
                    .into()
            }
            LogFormat::Bare => {
                let indicatif_layer = tracing_indicatif::IndicatifLayer::new();
                registry
                    .with(
                        fmt_layer()
                            .compact()
                            .with_span_events(FmtSpan::NONE)
                            .without_time()
                            .with_target(false)
                            .with_ansi(false)
                            .with_writer(indicatif_layer.get_stderr_writer()),
                    )
                    .with(indicatif_layer)
                    .into()
            }
            LogFormat::Json => {
                let indicatif_layer = tracing_indicatif::IndicatifLayer::new();
                registry
                    .with(
                        fmt_layer()
                            .json()
                            .with_span_events(FmtSpan::NONE)
                            .with_writer(indicatif_layer.get_stderr_writer()),
                    )
                    .with(indicatif_layer)
                    .into()
            }
        };
        // Uses `tracing::dispatcher::set_global_default` directly instead of
        // `SubscriberInitExt::init()`, because the latter also calls `tracing_log::LogTracer::init()
        tracing::dispatcher::set_global_default(dispatch)
            .expect("failed to set global default subscriber");
    }
}

impl Default for LogFormat {
    fn default() -> Self {
        if cfg!(debug_assertions) {
            Self::Pretty
        } else {
            Self::Compact
        }
    }
}

impl FromStr for LogFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "full" => Ok(Self::Full),
            "compact" => Ok(Self::Compact),
            "pretty" | "verbose" => Ok(Self::Pretty),
            "bare" => Ok(Self::Bare),
            "json" | "jsonl" => Ok(Self::Json),
            _ => Err(format!(
                "Invalid log format '{s}'. Valid options: json, full, compact, bare or pretty"
            )),
        }
    }
}

/// Initialize the log -> tracing bridge.
///
/// This should be called once after setting up the tracing subscriber.
fn init_log_bridge(env_filter: &EnvFilter) {
    let mut log_builder = tracing_log::LogTracer::builder()
        .with_interest_cache(tracing_log::InterestCacheConfig::default());
    if let Some(Some(max_level)) = env_filter.max_level_hint().map(LevelFilter::into_level) {
        let max_level = match max_level {
            Level::DEBUG => log::LevelFilter::Debug,
            Level::INFO => log::LevelFilter::Info,
            Level::WARN => log::LevelFilter::Warn,
            Level::ERROR => log::LevelFilter::Error,
            Level::TRACE => log::LevelFilter::Trace,
        };
        log_builder = log_builder.with_max_level(max_level);
    }
    log_builder
        .init()
        .expect("failed to initialize log -> tracing bridge: LogTracer already set");
}

/// Initialize the global tracing subscriber for the given filter and format.
///
/// This function:
/// 1. Bridges `log` records into `tracing` events for compatibility
/// 2. Uses the provided filter string for log filtering
/// 3. Uses the provided format for output
/// 4. Sets up the global tracing subscriber
/// 5. Optionally includes `IndicatifLayer` for progress bar support
pub fn init_tracing(filter: &str, format: Option<String>, use_progress: bool) {
    // Set up the filter from the provided string
    let env_filter = EnvFilter::from_str(filter).unwrap_or_else(|_| {
      eprintln!("Warning: Invalid filter string '{filter}' passed. Since you passed a filter, you likely want to debug us, so we set the filter to debug");
      EnvFilter::new("debug")
    });

    // Determine the format
    let log_format = format
        .and_then(|s| {
            s.parse::<LogFormat>()
                .map_err(|e| {
                    eprintln!("Warning: {e}");
                    eprintln!(
                        "Falling back to default format ({:?})",
                        LogFormat::default()
                    );
                })
                .ok()
        })
        .unwrap_or_default();

    // Initialize log -> tracing bridge
    init_log_bridge(&env_filter);
    if use_progress {
        log_format.init_with_progress(env_filter);
    } else {
        log_format.init(env_filter);
    }
}

/// Ensures that the log level for `martin_core` matches the log level for `replacement`.
#[must_use]
pub fn ensure_martin_core_log_level_matches(
    env_filter: Option<String>,
    replacement: &'static str,
) -> String {
    if let Some(rust_log) = env_filter {
        // If RUST_LOG is set and contains replacement (e.g., martin=) but not martin_core=, mirror the level
        if rust_log.contains(replacement) && !rust_log.contains("martin_core=") {
            if let Some(level) = rust_log
                .split(',')
                .find_map(|s| s.strip_prefix(replacement))
            {
                format!("{rust_log},martin_core={level}")
            } else {
                rust_log
            }
        } else {
            rust_log
        }
    } else {
        format!("{replacement}info,martin_core=info")
    }
}
