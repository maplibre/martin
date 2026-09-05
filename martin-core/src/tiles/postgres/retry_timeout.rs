//! How long the first `PostgreSQL` connection is retried before Martin gives up.

use std::fmt;
use std::str::FromStr;
use std::time::Duration;

use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// How long the first `PostgreSQL` connection is retried before startup fails.
///
/// Configured as a duration like `30s` or the literal `infinite`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RetryTimeout {
    /// Retry until this much time has passed, so `0s` fails on the first refused connection.
    After(Duration),
    /// Retry until the database answers.
    Infinite,
}

impl RetryTimeout {
    /// Whether another attempt is allowed after `elapsed` time spent retrying.
    #[must_use]
    pub fn allows(self, elapsed: Duration) -> bool {
        match self {
            Self::After(timeout) => elapsed < timeout,
            Self::Infinite => true,
        }
    }
}

impl Default for RetryTimeout {
    fn default() -> Self {
        Self::After(Duration::from_secs(30))
    }
}

impl fmt::Display for RetryTimeout {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::After(timeout) => humantime::format_duration(*timeout).fmt(f),
            Self::Infinite => f.write_str("infinite"),
        }
    }
}

impl FromStr for RetryTimeout {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        if s.eq_ignore_ascii_case("infinite") {
            return Ok(Self::Infinite);
        }
        humantime::parse_duration(s)
            .map(Self::After)
            .map_err(|_not_a_duration| {
                format!("expected a duration like `30s` or `infinite`, got {s:?}")
            })
    }
}

impl Serialize for RetryTimeout {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for RetryTimeout {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct RetryTimeoutVisitor;

        impl Visitor<'_> for RetryTimeoutVisitor {
            type Value = RetryTimeout;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a duration like `30s` or `infinite`")
            }

            fn visit_str<E: de::Error>(self, s: &str) -> Result<Self::Value, E> {
                s.parse().map_err(E::custom)
            }
        }

        deserializer.deserialize_str(RetryTimeoutVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_duration_or_infinite() {
        assert_eq!("0s".parse::<RetryTimeout>(), Ok(RetryTimeout::After(Duration::ZERO)));
        assert_eq!(
            "1m 30s".parse::<RetryTimeout>(),
            Ok(RetryTimeout::After(Duration::from_secs(90)))
        );
        assert_eq!("Infinite".parse::<RetryTimeout>(), Ok(RetryTimeout::Infinite));
        "soon".parse::<RetryTimeout>().unwrap_err();
        "30".parse::<RetryTimeout>().unwrap_err();
        assert_eq!(
            serde_json::from_str::<RetryTimeout>("\"3s\"").unwrap(),
            RetryTimeout::After(Duration::from_secs(3))
        );
        assert_eq!(
            serde_json::from_str::<RetryTimeout>("\"infinite\"").unwrap(),
            RetryTimeout::Infinite
        );
        assert_eq!(
            serde_json::from_str::<RetryTimeout>("30")
                .unwrap_err()
                .to_string(),
            "invalid type: integer `30`, expected a duration like `30s` or `infinite` at line 1 column 2"
        );
        assert_eq!(
            serde_json::to_string(&RetryTimeout::After(Duration::from_secs(90))).unwrap(),
            "\"1m 30s\""
        );
        assert_eq!(serde_json::to_string(&RetryTimeout::Infinite).unwrap(), "\"infinite\"");
    }

    #[test]
    fn allows_compares_the_time_spent() {
        assert!(!RetryTimeout::After(Duration::ZERO).allows(Duration::ZERO));
        assert!(RetryTimeout::After(Duration::from_secs(2)).allows(Duration::from_secs(1)));
        assert!(!RetryTimeout::After(Duration::from_secs(2)).allows(Duration::from_secs(2)));
        assert!(RetryTimeout::Infinite.allows(Duration::MAX));
    }
}
