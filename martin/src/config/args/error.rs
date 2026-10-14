//! Failures from parsing and reconciling command-line arguments.
//!
//! Produced only by [`config::args`](crate::config::args), consumed only by `main`.

use std::fmt::Write as _;

/// A convenience [`Result`] for command-line argument handling.
pub type ArgsResult<T> = Result<T, ArgsError>;

/// Why the given command-line arguments could not be turned into a config.
#[derive(thiserror::Error, Debug)]
pub enum ArgsError {
    #[error("The --config and the connection parameters cannot be used together. Please remove unsupported parameters '{}'", elide_vec(.0, 3, 15))]
    ConfigAndConnections(Vec<String>),

    #[error("Unrecognizable connection strings: {0:?}")]
    UnrecognizableConnections(Vec<String>),
}

fn elide_vec(vec: &[String], max_items: usize, max_len: usize) -> String {
    let mut s = String::new();
    for (i, v) in vec.iter().enumerate() {
        if i > max_items {
            let _ = write!(s, " and {} more", vec.len() - i);
            break;
        }
        if i > 0 {
            s.push(' ');
        }
        if v.len() > max_len {
            let mut bytes = 0usize;
            s.extend(v.chars().take_while(|c| {
                bytes += c.len_utf8();
                bytes <= max_len
            }));
            s.push('…');
        } else {
            s.push_str(v);
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case::empty(&[], "")]
    #[case::one(&["a"], "a")]
    #[case::spaced(&["a", "b", "c"], "a b c")]
    #[case::truncated(&["0123456789abcdefXYZ"], "0123456789abcde…")]
    #[case::utf8_boundary(&["ääääääääää"], "äääääää…")]
    #[case::elided(&["1", "2", "3", "4", "5", "6", "7"], "1 2 3 4 and 3 more")]
    fn elide_vec_shortens_long_lists(#[case] input: &[&str], #[case] expected: &str) {
        let input: Vec<String> = input.iter().map(ToString::to_string).collect();
        assert_eq!(elide_vec(&input, 3, 15), expected);
    }

    #[test]
    fn config_and_connections_lists_the_offending_parameters() {
        let err = ArgsError::ConfigAndConnections(vec![
            "postgres://x".to_owned(),
            "b.mbtiles".to_owned(),
        ]);
        assert_eq!(
            err.to_string(),
            "The --config and the connection parameters cannot be used together. Please remove unsupported parameters 'postgres://x b.mbtiles'"
        );
        let err = ArgsError::UnrecognizableConnections(vec!["what".to_owned()]);
        assert_eq!(
            err.to_string(),
            r#"Unrecognizable connection strings: ["what"]"#
        );
    }
}
