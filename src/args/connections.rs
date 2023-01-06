use crate::Error;

#[derive(Debug, Clone)]
pub enum State<T: Clone> {
    Ignore,
    Share(T),
    Take(T),
}

#[derive(Debug)]
pub struct Connections {
    state: Vec<State<()>>,
    values: Vec<String>,
}

impl Connections {
    #[must_use]
    pub fn new(values: Vec<String>) -> Self {
        Self {
            state: vec![State::Ignore; values.len()],
            values,
        }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

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
            if matches!(self.state[i], State::Take(_)) {
                continue;
            }
            let state = handle(name);
            self.state[i] = match state {
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
    pub fn check(self) -> Result<(), Error> {
        let mut unrecognized = Vec::new();
        for (i, value) in self.values.into_iter().enumerate() {
            if let State::Ignore = self.state[i] {
                unrecognized.push(value);
            }
        }
        if unrecognized.is_empty() {
            Ok(())
        } else {
            Err(Error::UnrecognizableConnections(unrecognized))
        }
    }
}
