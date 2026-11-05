use crate::config::args::{ArgsError, ArgsResult};

#[derive(Debug, Clone)]
pub enum State<T: Clone> {
    Ignore,
    Share(T),
    Take(T),
}

#[derive(Debug)]
pub struct Arguments {
    state: Vec<State<()>>,
    values: Vec<String>,
}

impl Arguments {
    #[must_use]
    pub fn new(values: Vec<String>) -> Self {
        Self {
            state: vec![State::Ignore; values.len()],
            values,
        }
    }

    #[cfg(any(
        feature = "postgres",
        feature = "mbtiles",
        feature = "pmtiles",
        feature = "processing",
        feature = "cog",
        feature = "unstable-duckdb"
    ))]
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    #[cfg(any(
        feature = "postgres",
        feature = "mbtiles",
        feature = "pmtiles",
        feature = "processing",
        feature = "cog",
        feature = "unstable-duckdb"
    ))]
    /// Process input params in the original order, but only if no prior consumer has claimed it.
    /// Each consumer can either take it (no other consumer will see it),
    /// share it (other consumers will see it too),
    /// or ignore it (same as sharing, but it is an error if no consumer claims it later)
    pub fn process<T, F>(&mut self, mut handle: F) -> Vec<T>
    where
        T: Clone,
        F: FnMut(&str) -> State<T>,
    {
        let mut result = Vec::new();
        for (i, name) in self.values.iter().enumerate() {
            if matches!(self.state[i], State::Take(())) {
                continue;
            }
            let state = handle(name);
            self.state[i] = match state {
                // A later consumer ignoring a path keeps an earlier consumer's share.
                State::Ignore if matches!(self.state[i], State::Share(())) => State::Share(()),
                State::Ignore => State::Ignore,
                State::Share(v) => {
                    result.push(v);
                    State::Share(())
                }
                State::Take(v) => {
                    result.push(v);
                    State::Take(())
                }
            }
        }
        result
    }

    /// Check that all params have been claimed
    pub fn check(self) -> ArgsResult<()> {
        let mut unrecognized = Vec::new();
        for (i, value) in self.values.into_iter().enumerate() {
            if matches!(self.state[i], State::Ignore) {
                unrecognized.push(value);
            }
        }
        if unrecognized.is_empty() {
            Ok(())
        } else {
            Err(ArgsError::UnrecognizableConnections(unrecognized))
        }
    }
}

#[cfg(test)]
#[cfg(any(
    feature = "postgres",
    feature = "mbtiles",
    feature = "pmtiles",
    feature = "processing",
    feature = "cog",
    feature = "unstable-duckdb"
))]
mod tests {
    use std::assert_matches;

    use super::*;

    #[test]
    fn shared_path_stays_claimed_when_a_later_consumer_ignores_it() {
        let mut args = Arguments::new(vec!["tiles/".to_owned()]);
        let shared = args.process(|v| State::Share(v.to_owned()));
        let ignored: Vec<String> = args.process(|_| State::Ignore);
        assert_eq!(shared, vec!["tiles/"]);
        assert!(ignored.is_empty());
        args.check().unwrap();
    }

    #[test]
    fn ignored_path_stays_claimed_when_a_later_consumer_shares_it() {
        let mut args = Arguments::new(vec!["tiles/".to_owned()]);
        let ignored: Vec<String> = args.process(|_| State::Ignore);
        let shared = args.process(|v| State::Share(v.to_owned()));
        assert!(ignored.is_empty());
        assert_eq!(shared, vec!["tiles/"]);
        args.check().unwrap();
    }

    #[test]
    fn path_every_consumer_ignores_is_unrecognized() {
        let mut args = Arguments::new(vec!["tiles/".to_owned()]);
        let first: Vec<String> = args.process(|_| State::Ignore);
        let second: Vec<String> = args.process(|_| State::Ignore);
        assert!(first.is_empty());
        assert!(second.is_empty());
        assert_matches!(
            args.check(),
            Err(ArgsError::UnrecognizableConnections(v)) if v == vec!["tiles/"]
        );
    }
}
