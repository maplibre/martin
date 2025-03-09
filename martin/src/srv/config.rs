use serde::{Deserialize, Serialize};
use martin_observability_utils::LogFormatOptions;
use crate::args::PreferredEncoding;

pub const KEEP_ALIVE_DEFAULT: u64 = 75;
pub const LISTEN_ADDRESSES_DEFAULT: &str = "0.0.0.0:3000";

#[serde_with::skip_serializing_none]
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
pub struct SrvConfig {
    pub keep_alive: Option<u64>,
    pub listen_addresses: Option<String>,
    pub base_path: Option<String>,
    pub worker_processes: Option<usize>,
    pub preferred_encoding: Option<PreferredEncoding>,
    #[cfg(feature = "webui")]
    pub web_ui: Option<crate::args::WebUiMode>,
    /// How to format the logs. [DEFAULT: compact]
    // ! log_format is never actually used from here (instead done as the first thing in initialisation).
    // ! We need tracing to raise errors/warnings during parsing configuration options.
    // ! This is to not have unexpected values !
    pub log_format: Option<LogFormatOptions>,
    /// Set which logs martin outputs. [DEFAULT: martin=info]
    /// See [here](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/filter/struct.EnvFilter.html#example-syntax) for more information.
    // ! log_level is never actually used from here (instead done as the first thing in initialisation).
    // ! We need tracing to raise errors/warnings during parsing configuration options.
    // ! This is to not have unexpected values !
    pub log_level: Option<String>,
}

#[cfg(test)]
mod tests {
    use indoc::indoc;

    use super::*;
    use crate::test_utils::some;

    #[test]
    fn parse_config() {
        assert_eq!(
            serde_yaml::from_str::<SrvConfig>(indoc! {"
                keep_alive: 75
                listen_addresses: '0.0.0.0:3000'
                worker_processes: 8
            "})
            .unwrap(),
            SrvConfig {
                keep_alive: Some(75),
                listen_addresses: some("0.0.0.0:3000"),
                worker_processes: Some(8),
                ..Default::default()
            }
        );
        assert_eq!(
            serde_yaml::from_str::<SrvConfig>(indoc! {"
                keep_alive: 75
                listen_addresses: '0.0.0.0:3000'
                worker_processes: 8
                preferred_encoding: br
            "})
            .unwrap(),
            SrvConfig {
                keep_alive: Some(75),
                listen_addresses: some("0.0.0.0:3000"),
                worker_processes: Some(8),
                preferred_encoding: Some(PreferredEncoding::Brotli),
                ..Default::default()
            }
        );
        assert_eq!(
            serde_yaml::from_str::<SrvConfig>(indoc! {"
                keep_alive: 75
                listen_addresses: '0.0.0.0:3000'
                worker_processes: 8
                preferred_encoding: brotli
            "})
            .unwrap(),
            SrvConfig {
                keep_alive: Some(75),
                listen_addresses: some("0.0.0.0:3000"),
                worker_processes: Some(8),
                preferred_encoding: Some(PreferredEncoding::Brotli),
                ..Default::default()
            }
        );
    }
}
