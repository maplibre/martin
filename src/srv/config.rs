use serde::{Deserialize, Serialize};

pub const KEEP_ALIVE_DEFAULT: u64 = 75;
pub const LISTEN_ADDRESSES_DEFAULT: &str = "0.0.0.0:3000";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
pub struct SrvConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keep_alive: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub listen_addresses: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worker_processes: Option<usize>,
}

#[cfg(test)]
mod tests {
    use indoc::indoc;

    use super::*;
    use crate::test_utils::some;

    #[test]
    fn parse_empty_config() {
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
            }
        );
    }
}
