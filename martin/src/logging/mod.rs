//! Logging initialization for Martin using `tracing` and `tracing-subscriber`.
//!
//! This module provides static logging configuration controlled by:
//! - [`EnvFilter`]: Controls log level filtering (standard tracing-subscriber behavior)
//! - [`LogFormat`]: Controls output format (json, full, compact, bare, pretty)

use std::str::FromStr;

use tracing::level_filters::LevelFilter;
use tracing::{Dispatch, Level};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::layer::SubscriberExt as _;

pub mod progress;

/// Log output format options.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
        tracing::dispatcher::set_global_default(self.dispatch(env_filter))
            .expect("failed to set global default subscriber");
    }

    /// The subscriber for the selected format, writing to stdout.
    fn dispatch(self, env_filter: EnvFilter) -> Dispatch {
        match self {
            Self::Full => tracing_subscriber::fmt()
                .with_span_events(FmtSpan::NONE)
                .with_env_filter(env_filter)
                .finish()
                .into(),
            Self::Compact => tracing_subscriber::fmt()
                .compact()
                .with_span_events(FmtSpan::NONE)
                .with_env_filter(env_filter)
                .finish()
                .into(),
            Self::Pretty => tracing_subscriber::fmt()
                .pretty()
                .with_env_filter(env_filter)
                .finish()
                .into(),
            Self::Bare => tracing_subscriber::fmt()
                .compact()
                .with_span_events(FmtSpan::NONE)
                .without_time()
                .with_target(false)
                .with_ansi(false)
                .with_env_filter(env_filter)
                .finish()
                .into(),
            Self::Json => tracing_subscriber::fmt()
                .json()
                .with_span_events(FmtSpan::NONE)
                .with_env_filter(env_filter)
                .finish()
                .into(),
        }
    }

    /// The subscriber for the selected format, writing without colors into `writer`.
    #[cfg(feature = "tui")]
    fn dispatch_into<W>(self, env_filter: EnvFilter, writer: W) -> Dispatch
    where
        W: for<'w> tracing_subscriber::fmt::MakeWriter<'w> + Send + Sync + 'static,
    {
        let builder = tracing_subscriber::fmt()
            .with_span_events(FmtSpan::NONE)
            .with_ansi(false)
            .with_writer(writer)
            .with_env_filter(env_filter);
        match self {
            Self::Full => builder.finish().into(),
            Self::Compact => builder.compact().finish().into(),
            Self::Pretty => builder.pretty().finish().into(),
            Self::Bare => builder.compact().without_time().with_target(false).finish().into(),
            Self::Json => builder.json().finish().into(),
        }
    }

    /// Initialize logging according to the selected format with a progress bar.
    ///
    /// Uses `tracing::dispatcher::set_global_default` directly instead of
    /// `SubscriberInitExt::init()` for the same reason as [`Self::init`]:
    /// to prevent `tracing-subscriber`'s `tracing-log` feature from installing
    /// its own `LogTracer`, which would conflict with `init_log_bridge`.
    pub fn init_with_progress(self, env_filter: EnvFilter) {
        tracing::dispatcher::set_global_default(self.dispatch_with_progress(env_filter))
            .expect("failed to set global default subscriber");
    }

    /// The subscriber for the selected format, writing to stderr around an indicatif progress bar.
    fn dispatch_with_progress(self, env_filter: EnvFilter) -> Dispatch {
        use tracing_subscriber::fmt::layer as fmt_layer;

        let registry = tracing_subscriber::registry().with(env_filter);

        // code below looks duplicated, but it has to be this way due to how types currently work.
        // maybe there is a better way that I can not see
        match self {
            Self::Full => {
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
            Self::Compact => {
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
            Self::Pretty => {
                let indicatif_layer = tracing_indicatif::IndicatifLayer::new();
                registry
                    .with(fmt_layer().pretty().with_writer(indicatif_layer.get_stderr_writer()))
                    .with(indicatif_layer)
                    .into()
            }
            Self::Bare => {
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
            Self::Json => {
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
        }
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

impl LogFormat {
    /// Read `RUST_LOG_FORMAT` and resolve it to a [`LogFormat`].
    ///
    /// Falls back to [`LogFormat::default`] when the variable is unset, empty, or invalid.
    /// On invalid values, a warning is written to stderr.
    #[must_use]
    pub fn from_env() -> Self {
        Self::from_env_or(Self::default())
    }

    /// Read `RUST_LOG_FORMAT` and resolve it to a [`LogFormat`].
    ///
    /// Falls back to `default` when the variable is unset, empty, or invalid.
    /// On invalid values, a warning is written to stderr.
    #[must_use]
    #[expect(
        clippy::print_stderr,
        reason = "tracing subscriber not yet initialized at this point"
    )]
    pub fn from_env_or(default: Self) -> Self {
        match std::env::var("RUST_LOG_FORMAT").ok().as_deref() {
            None | Some("") => default,
            Some(s) => s.parse().unwrap_or_else(|e| {
                eprintln!("Warning: {e}");
                eprintln!("Falling back to default format ({default:?})");
                default
            }),
        }
    }

    /// Returns `true` if this format is JSON (`json` / `jsonl`).
    #[must_use]
    pub const fn is_json(self) -> bool {
        matches!(self, Self::Json)
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
    let mut log_builder =
        tracing_log::LogTracer::builder().with_interest_cache(tracing_log::InterestCacheConfig::default());
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
pub fn init_tracing(filter: &str, log_format: LogFormat, use_progress: bool) {
    let env_filter = parse_filter(filter);

    // Initialize log -> tracing bridge
    init_log_bridge(&env_filter);
    if use_progress {
        log_format.init_with_progress(env_filter);
    } else {
        log_format.init(env_filter);
    }
}

/// The library crates that log at the level of `martin`, unless `RUST_LOG` sets their own.
const MIRRORED_CRATES: [&str; 2] = ["martin_core", "cog_tiles"];

/// Ensures that the log levels of [`MIRRORED_CRATES`] match the level `RUST_LOG` gives `directive_prefix`, e.g. `martin=`.
#[must_use]
pub fn ensure_library_log_levels_match(env_filter: Option<String>, directive_prefix: &'static str) -> String {
    let Some(rust_log) = env_filter else {
        let mut filter = format!("{directive_prefix}info");
        filter.extend(MIRRORED_CRATES.map(|krate| format!(",{krate}=info")));
        return filter;
    };
    let Some(level) = rust_log.split(',').find_map(|s| s.strip_prefix(directive_prefix)) else {
        return rust_log;
    };
    let unset = MIRRORED_CRATES
        .into_iter()
        .filter(|krate| !rust_log.split(',').any(|s| s.starts_with(&format!("{krate}="))))
        .map(|krate| format!(",{krate}={level}"));
    let mut filter = rust_log.clone();
    filter.extend(unset);
    filter
}

/// The filter for `filter`, or `debug` with a warning when it does not parse.
#[expect(
    clippy::print_stderr,
    reason = "tracing subscriber not yet initialized at this point"
)]
fn parse_filter(filter: &str) -> EnvFilter {
    EnvFilter::from_str(filter).unwrap_or_else(|_| {
        eprintln!("Warning: Invalid filter string '{filter}' passed. Since you passed a filter, you likely want to debug us, so we set the filter to debug");
        EnvFilter::new("debug")
    })
}

/// Initialize the global tracing subscriber writing `log_format` lines without colors into `writer`.
#[cfg(feature = "tui")]
pub fn init_tracing_into<W>(filter: &str, log_format: LogFormat, writer: W)
where
    W: for<'w> tracing_subscriber::fmt::MakeWriter<'w> + Send + Sync + 'static,
{
    let env_filter = parse_filter(filter);
    init_log_bridge(&env_filter);
    tracing::dispatcher::set_global_default(log_format.dispatch_into(env_filter, writer))
        .expect("failed to set global default subscriber");
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case(LogFormat::Full)]
    #[case(LogFormat::Compact)]
    #[case(LogFormat::Pretty)]
    #[case(LogFormat::Bare)]
    #[case(LogFormat::Json)]
    fn every_format_builds_a_subscriber(#[case] format: LogFormat) {
        let dispatch = format.dispatch(EnvFilter::new("info"));
        tracing::dispatcher::with_default(&dispatch, || {
            tracing::info!(answer = 42, "hello from {format:?}");
            tracing::trace!("filtered out");
        });

        let dispatch = format.dispatch_with_progress(EnvFilter::new("debug"));
        tracing::dispatcher::with_default(&dispatch, || {
            tracing::debug!("hello from {format:?} with progress");
        });
    }

    #[rstest]
    #[case("full", "Full")]
    #[case("COMPACT", "Compact")]
    #[case("pretty", "Pretty")]
    #[case("verbose", "Pretty")]
    #[case("bare", "Bare")]
    #[case("json", "Json")]
    #[case("jsonl", "Json")]
    fn formats_parse_case_insensitively(#[case] input: &str, #[case] expected: &str) {
        let format: LogFormat = input.parse().expect("a known format");
        assert_eq!(format!("{format:?}"), expected);
        assert_eq!(format.is_json(), expected == "Json");
    }

    #[test]
    fn unknown_format_is_rejected() {
        let err = "xml".parse::<LogFormat>().unwrap_err();
        assert_eq!(
            err,
            "Invalid log format 'xml'. Valid options: json, full, compact, bare or pretty"
        );
    }

    #[test]
    fn default_format_depends_on_the_build_profile() {
        let default = format!("{:?}", LogFormat::default());
        assert_eq!(default, if cfg!(debug_assertions) { "Pretty" } else { "Compact" });
    }

    #[test]
    fn unset_rust_log_logs_every_crate_at_info() {
        let actual = ensure_library_log_levels_match(None, "martin=");
        assert_eq!(actual, "martin=info,martin_core=info,cog_tiles=info");
    }

    #[test]
    fn martin_level_is_mirrored_to_the_libraries() {
        let actual = ensure_library_log_levels_match(Some("martin=debug".to_owned()), "martin=");
        assert_eq!(actual, "martin=debug,martin_core=debug,cog_tiles=debug");
    }

    #[test]
    fn martin_level_is_mirrored_from_anywhere_in_the_list() {
        let actual = ensure_library_log_levels_match(Some("actix=warn,martin=trace".to_owned()), "martin=");
        assert_eq!(actual, "actix=warn,martin=trace,martin_core=trace,cog_tiles=trace");
    }

    #[test]
    fn library_levels_set_in_rust_log_are_kept() {
        let actual = ensure_library_log_levels_match(
            Some("martin=debug,martin_core=warn,cog_tiles=error".to_owned()),
            "martin=",
        );
        assert_eq!(actual, "martin=debug,martin_core=warn,cog_tiles=error");
    }

    #[test]
    fn only_libraries_without_a_level_get_martins() {
        let actual = ensure_library_log_levels_match(Some("martin=debug,martin_core=warn".to_owned()), "martin=");
        assert_eq!(actual, "martin=debug,martin_core=warn,cog_tiles=debug");
    }

    #[test]
    fn library_with_a_longer_name_does_not_count_as_set() {
        let actual = ensure_library_log_levels_match(Some("martin=debug,xmartin_core=warn".to_owned()), "martin=");
        assert_eq!(
            actual,
            "martin=debug,xmartin_core=warn,martin_core=debug,cog_tiles=debug"
        );
    }

    #[test]
    fn rust_log_without_a_martin_level_is_unchanged() {
        let actual = ensure_library_log_levels_match(Some("actix=warn".to_owned()), "martin=");
        assert_eq!(actual, "actix=warn");
    }

    #[test]
    fn bare_martin_is_not_a_level() {
        let actual = ensure_library_log_levels_match(Some("martin".to_owned()), "martin=");
        assert_eq!(actual, "martin");
    }

    #[test]
    fn crate_with_a_longer_name_is_not_martin() {
        let actual = ensure_library_log_levels_match(Some("xmartin=debug".to_owned()), "martin=");
        assert_eq!(actual, "xmartin=debug");
    }

    #[test]
    fn invalid_filter_falls_back_to_debug() {
        assert_eq!(parse_filter("info").to_string(), "info");
        assert_eq!(parse_filter("=!!=").to_string(), "debug");
    }
}
