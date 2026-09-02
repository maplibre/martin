//! How many times the first `PostgreSQL` connection is retried before Martin gives up.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// How many times the first `PostgreSQL` connection is retried, one second apart, before startup fails.
///
/// Configured as a number or the literal `infinite`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnectionRetries {
    /// Retry this many times, so `0` fails on the first refused connection.
    Count(u32),
    /// Retry until the database answers.
    Infinite,
}

impl ConnectionRetries {
    /// Retries before startup fails when nothing is configured.
    pub const DEFAULT: Self = Self::Count(30);

    /// Whether another attempt is allowed after `failed` failed ones.
    #[must_use]
    pub const fn allows(self, failed: u32) -> bool {
        match self {
            Self::Count(max) => failed < max,
            Self::Infinite => true,
        }
    }
}

impl Default for ConnectionRetries {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl fmt::Display for ConnectionRetries {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Count(n) => write!(f, "{n}"),
            Self::Infinite => f.write_str("infinite"),
        }
    }
}

impl FromStr for ConnectionRetries {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        if s.eq_ignore_ascii_case("infinite") {
            return Ok(Self::Infinite);
        }
        s.parse::<u32>().map(Self::Count).map_err(|_not_a_number| {
            format!("expected a number of retries or `infinite`, got {s:?}")
        })
    }
}

impl Serialize for ConnectionRetries {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Count(n) => serializer.serialize_u32(*n),
            Self::Infinite => serializer.serialize_str("infinite"),
        }
    }
}

impl<'de> Deserialize<'de> for ConnectionRetries {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Count(u32),
            Text(String),
        }
        match Raw::deserialize(deserializer)? {
            Raw::Count(n) => Ok(Self::Count(n)),
            Raw::Text(s) => s.parse().map_err(serde::de::Error::custom),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_count_or_infinite() {
        assert_eq!(
            "0".parse::<ConnectionRetries>(),
            Ok(ConnectionRetries::Count(0))
        );
        assert_eq!(
            "7".parse::<ConnectionRetries>(),
            Ok(ConnectionRetries::Count(7))
        );
        assert_eq!(
            "Infinite".parse::<ConnectionRetries>(),
            Ok(ConnectionRetries::Infinite)
        );
        "soon".parse::<ConnectionRetries>().unwrap_err();
        assert_eq!(
            serde_json::from_str::<ConnectionRetries>("3").unwrap(),
            ConnectionRetries::Count(3)
        );
        assert_eq!(
            serde_json::from_str::<ConnectionRetries>("\"infinite\"").unwrap(),
            ConnectionRetries::Infinite
        );
        assert_eq!(
            serde_json::to_string(&ConnectionRetries::Infinite).unwrap(),
            "\"infinite\""
        );
    }

    #[test]
    fn allows_counts_failed_attempts() {
        assert!(!ConnectionRetries::Count(0).allows(0));
        assert!(ConnectionRetries::Count(2).allows(1));
        assert!(!ConnectionRetries::Count(2).allows(2));
        assert!(ConnectionRetries::Infinite.allows(u32::MAX));
    }
}
