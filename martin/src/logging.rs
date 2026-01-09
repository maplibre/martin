//! Logging initialization for Martin using `tracing` and `tracing-subscriber`.
//!
//! This module provides static (non-reloadable) logging configuration controlled by:
//! - `RUST_LOG`: Controls log level filtering (standard tracing-subscriber behavior)
//! - `MARTIN_FORMAT`: Controls output format (compact, full, pretty, json)

use std::str::FromStr;

use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer, Registry};

/// Log output format options.
///
/// Controlled by the `MARTIN_FORMAT` environment variable.
#[derive(Debug, Clone, Copy)]
pub enum LogFormat {
    /// Emit human-readable, single-line logs.
    /// See [format::Full](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/fmt/format/struct.Full.html#example-output)
    Full,

    /// A variant of the full-format, optimized for short line lengths (default).
    /// See [format::Compact](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/fmt/format/struct.Compact.html#example-output)
    Compact,

    /// Excessively pretty, multi-line logs for local development/debugging.
    /// See [format::Pretty](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/fmt/format/struct.Pretty.html#example-output)
    Pretty,

    /// Output newline-delimited (structured) JSON logs.
    /// See [format::Json](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/fmt/format/struct.Json.html#example-output)
    Json,
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
            "json" | "jsonl" => Ok(Self::Json),
            _ => Err(format!(
                "Invalid log format '{s}'. Valid options: full, compact, pretty, json"
            )),
        }
    }
}

/// Initialize the global tracing subscriber for the given filter and format.
///
/// This function:
/// 1. Bridges `log` records into `tracing` events for compatibility
/// 2. Uses the provided filter string for log filtering
/// 3. Uses the provided format for output
/// 4. Sets up the global tracing subscriber
pub fn init_tracing(filter: &str, format: Option<String>) {
    // Initialize log -> tracing bridge (ignore if already initialized)
    let _ = tracing_log::LogTracer::builder()
        .with_interest_cache(tracing_log::InterestCacheConfig::default())
        .init()
        .expect("failed to intialise the global log -> tracing bridge");

    // Set up the filter from the provided string
    let env_filter = EnvFilter::from_str(filter).unwrap_or_else(|_| {
      eprintln!("Warning: Invalid filter string '{filter}' passed. Since you passed a filter, you likely want to debug us, so we set the filter to debug");
      EnvFilter::new("debug")
    });

    // Build and install the subscriber based on format
    let format = format
        .and_then(|s| {
            s.parse::<LogFormat>()
                .map_err(|e| {
                    eprintln!("Warning: {e}");
                    eprintln!("Falling back to default format (compact)");
                })
                .ok()
        })
        .unwrap_or_default();
    match format {
        LogFormat::Full => {
            let fmt_layer = tracing_subscriber::fmt::layer()
                .with_span_events(FmtSpan::NONE)
                .with_filter(env_filter);

            Registry::default().with(fmt_layer).init()
        }
        LogFormat::Compact => {
            let fmt_layer = tracing_subscriber::fmt::layer()
                .compact()
                .with_span_events(FmtSpan::NONE)
                .with_filter(env_filter);

            Registry::default().with(fmt_layer).init()
        }
        LogFormat::Pretty => {
            let fmt_layer = tracing_subscriber::fmt::layer()
                .pretty()
                .with_filter(env_filter);

            Registry::default().with(fmt_layer).init()
        }
        LogFormat::Json => {
            let fmt_layer = tracing_subscriber::fmt::layer()
                .json()
                .with_span_events(FmtSpan::NONE)
                .with_filter(env_filter);

            Registry::default().with(fmt_layer).init()
        }
    };
}

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

/// Initialize tracing for tests.
///
/// This is a simplified version that:
/// - Doesn't panic if already initialized (returns Ok/Err)
/// - Uses compact format
/// - Sets `is_test(true)` to avoid interference between tests
pub fn init_tracing_for_tests() -> Result<(), Box<dyn std::error::Error>> {
    use tracing_subscriber::fmt;

    let env_filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::from_str("info"))
        .unwrap();

    let subscriber = fmt()
        .compact()
        .with_test_writer()
        .with_env_filter(env_filter)
        .with_span_events(FmtSpan::NONE)
        .finish();

    tracing::subscriber::set_global_default(subscriber)?;
    Ok(())
}
