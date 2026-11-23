use crate::{Error, Expr};
use jiff::{fmt::temporal::Pieces, SignedDuration, Timestamp};
use std::cmp::Ordering;

const DAY: SignedDuration = SignedDuration::from_hours(24);
const SHYOFADAY: SignedDuration = DAY.checked_sub(SignedDuration::from_nanos(1)).unwrap();

/// Parses a timestamp from an already-decoded value.
///
/// The caller unquotes: a value that reached here through `Expr::Literal` carries no delimiters,
/// and stripping again would eat apostrophes that are part of the data.
fn parse_ts(s: &str) -> Result<Timestamp, Error> {
    let stripped = s.replace(' ', "T");
    let fromshort = match stripped.len() {
        4 => format!("{stripped}-01-01T00:00:00Z"),
        7 => format!("{stripped}-01T00:00:00Z"),
        10 => format!("{stripped}T00:00:00Z"),
        13 => format!("{stripped}:00:00Z"),
        16 => format!("{stripped}:00Z"),
        19 => format!("{stripped}Z"),
        _ => stripped,
    };

    let timestamp = fromshort.parse().map_err(Error::ParseTimestamp)?;
    // RFC 3339 ends a UTC offset at 23:59, and `jiff` reads one out to 25:59:59. An offset in
    // between would move the instant by a day or more, so it is refused, as `jiff` refuses 26:00.
    let offset = Pieces::parse(&fromshort)?.to_numeric_offset();
    if offset.is_some_and(|offset| i64::from(offset.seconds()).abs() >= DAY.as_secs()) {
        return Err(Error::TimestampOffsetOutOfRange(s.to_string()));
    }
    Ok(timestamp)
}

/// Parses a timestamp from an expression's cql2-text rendering, which is quoted.
fn parse_rendered_ts(expr: &Expr) -> Result<Timestamp, Error> {
    parse_ts(&crate::parser::strip_quotes(&expr.to_text()?))
}

/// Parses one bound of an interval.
///
/// OGC 21-065r2 admits `".."` for an unbounded bound (`$defs/intervalArray`), which stands for the
/// open end of the range rather than for any instant.
fn parse_bound(expr: &Expr, unbounded: Timestamp) -> Result<Timestamp, Error> {
    let text = crate::parser::strip_quotes(&expr.to_text()?).into_owned();
    if text == ".." {
        Ok(unbounded)
    } else {
        parse_ts(&text)
    }
}

/// Renders a timestamp literal in a single canonical spelling.
///
/// A timestamp denotes an instant, so `2012-08-10T05:30:00.000000Z` and `2012-08-10T05:30:00Z` are
/// the same value and must produce the same expression.
///
/// Only literals that already carry a time are rewritten. A date such as `2010-10-07` is left as
/// written, since widening it to an instant would change what the literal says. A literal that is
/// not a timestamp, an offset past 23:59 included, is left as written too, for the validator to
/// report.
pub(crate) fn canonical_timestamp(s: &str) -> String {
    let has_time = s.contains('T') || s.contains(' ');
    if !has_time || is_leap_second(s) {
        return s.to_string();
    }
    parse_ts(s).map_or_else(|_| s.to_string(), |ts| ts.to_string())
}

/// Whether a literal names a leap second.
///
/// `jiff` has no representation for one and rounds it down, so rewriting such a literal would move
/// the instant it names. It is left exactly as written instead.
fn is_leap_second(s: &str) -> bool {
    s.split(':').nth(2).is_some_and(|seconds| {
        seconds
            .strip_prefix("60")
            .is_some_and(|rest| !rest.starts_with(|c: char| c.is_ascii_digit()))
    })
}

/// Struct to hold a range of timestamps.
#[derive(Debug, Clone)]
pub struct DateRange {
    /// Start timestamp of the range
    pub start: Timestamp,
    /// End timestamp of the range
    pub end: Timestamp,
}

impl TryFrom<Expr> for DateRange {
    type Error = Error;
    fn try_from(v: Expr) -> Result<DateRange, Error> {
        match v {
            Expr::Interval { interval } => {
                let start = parse_bound(&interval[0], Timestamp::MIN)?;
                let end = parse_bound(&interval[1], Timestamp::MAX)?;
                Ok(DateRange { start, end })
            }
            Expr::Timestamp { timestamp } => {
                let start: Timestamp = parse_rendered_ts(&timestamp)?;
                Ok(DateRange { start, end: start })
            }
            Expr::Date { date } => {
                let start: Timestamp = parse_rendered_ts(&date)?;
                let end: Timestamp = start + SHYOFADAY;
                Ok(DateRange { start, end })
            }
            Expr::Literal(v) => {
                let start: Timestamp = parse_ts(&v)?;
                Ok(DateRange { start, end: start })
            }
            _ => Err(Error::ExprToDateRange(v)),
        }
    }
}

/// Two DateRanges are equal if both their start and end timestamps match exactly.
impl PartialEq for DateRange {
    fn eq(&self, other: &Self) -> bool {
        self.start == other.start && self.end == other.end
    }
}
/// Ordering for DateRanges:
/// - Less if this range ends before the other range starts.
/// - Greater if this range starts after the other range ends.
/// - Equal if boundaries match exactly.
/// - None if ranges overlap without boundary precedence.
impl PartialOrd for DateRange {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        if self == other {
            Some(Ordering::Equal)
        } else if self.end < other.start {
            Some(Ordering::Less)
        } else if self.start > other.end {
            Some(Ordering::Greater)
        } else {
            None
        }
    }
}

/// Run a temporal operation.
pub fn temporal_op(left_expr: Expr, right_expr: Expr, op: &str) -> Result<Expr, Error> {
    // Accept any spelling a caller might hold, then work in the schema's.
    let op = &crate::expr::canonical_op(op);
    let invop = match op.as_str() {
        "t_after" => "t_before",
        "t_metBy" => "t_meets",
        "t_overlappedBy" => "t_overlaps",
        "t_startedBy" => "t_starts",
        "t_contains" => "t_during",
        "t_finishedBy" => "t_finishes",
        _ => op,
    };

    let left = DateRange::try_from(left_expr)?;
    let right = DateRange::try_from(right_expr)?;
    // Each inverse relation is its counterpart with the operands exchanged.
    let (left, right) = if invop == op {
        (left, right)
    } else {
        (right, left)
    };

    let out = match invop {
        "t_before" => left.end < right.start,
        "t_meets" => left.end == right.start,
        // `overlaps`: left begins first, the two share an interior, and left ends first. The first
        // conjunct compares the two starts; comparing left's start to right's *end* is implied by
        // the others and admits ranges that are contained rather than overlapping.
        "t_overlaps" => left.start < right.start && right.start < left.end && left.end < right.end,
        "t_starts" => left.start == right.start && left.end < right.end,
        "t_during" => left.start > right.start && left.end < right.end,
        "t_finishes" => left.start > right.start && left.end == right.end,
        "t_equals" => left.start == right.start && left.end == right.end,
        "t_disjoint" => !(left.start <= right.end && left.end >= right.start),
        "t_intersects" => left.start <= right.end && left.end >= right.start,
        _ => return Err(Error::OpNotImplemented("temporal")),
    };

    Ok(Expr::Bool(out))
}

#[cfg(test)]
mod tests {
    use super::{canonical_timestamp, DateRange};
    use crate::{Error, Expr};
    use serde_json::json;

    #[test]
    fn timestamp_math() {
        // https://github.com/developmentseed/cql2-rs/issues/66
        let expr: Expr = serde_json::from_value(json!({"date": "2020-02-18"})).unwrap();
        let _: DateRange = expr.try_into().unwrap();
    }

    /// RFC 3339 ends a UTC offset at 23:59, and `jiff` reads one out to 25:59:59. A literal in
    /// between is not a timestamp: it stays as written, where the validator reports it, and it
    /// names no instant to evaluate.
    #[test]
    fn an_offset_of_a_day_or_more_is_refused() {
        // https://github.com/developmentseed/cql2-rs/issues/273
        for offset in ["+24:00", "-24:00", "+25:00", "-25:59", "+25:59:59"] {
            let written = format!("2020-01-01T00:00:00{offset}");
            assert_eq!(
                canonical_timestamp(&written),
                written,
                "{written} should be left as written"
            );
            let timestamp = Expr::Timestamp {
                timestamp: Box::new(Expr::Literal(written.clone())),
            };
            assert!(
                matches!(
                    DateRange::try_from(timestamp),
                    Err(Error::TimestampOffsetOutOfRange(_))
                ),
                "{written} should not name an instant"
            );
        }
    }

    /// The check is shared by every path that reads a timestamp: an interval bound is refused the
    /// same way, and a comparison in `reduce` reports it.
    #[test]
    fn a_refused_offset_reaches_every_reader() {
        const WRITTEN: &str = "2020-01-01T00:00:00+25:00";
        let interval = Expr::Interval {
            interval: vec![
                Box::new(Expr::Literal(WRITTEN.to_string())),
                Box::new(Expr::Literal("..".to_string())),
            ],
        };
        assert!(matches!(
            DateRange::try_from(interval),
            Err(Error::TimestampOffsetOutOfRange(_))
        ));
        let comparison = crate::parse_text(&format!(
            "TIMESTAMP('{WRITTEN}') < TIMESTAMP('2021-01-01T00:00:00Z')"
        ))
        .unwrap();
        assert!(matches!(
            comparison.reduce(None),
            Err(Error::TimestampOffsetOutOfRange(_))
        ));
    }

    /// An offset under a day names an instant, which keeps its one UTC spelling.
    #[test]
    fn an_offset_under_a_day_is_converted_to_utc() {
        for (written, utc) in [
            ("2020-01-01T00:00:00+23:59", "2019-12-31T00:01:00Z"),
            ("2020-01-01T00:00:00-23:59", "2020-01-01T23:59:00Z"),
            ("2020-01-01T00:00:00+05:30", "2019-12-31T18:30:00Z"),
        ] {
            assert_eq!(canonical_timestamp(written), utc);
        }
    }

    /// Both encodings hand a refused timestamp to the validator unchanged.
    #[test]
    fn a_refused_offset_fails_validation_in_both_encodings() {
        const WRITTEN: &str = "2020-01-01T00:00:00+25:00";
        let from_text =
            crate::parse_text(&format!("t_before(datetime, TIMESTAMP('{WRITTEN}'))")).unwrap();
        let from_json: Expr = serde_json::from_value(json!({
            "op": "t_before",
            "args": [{"property": "datetime"}, {"timestamp": WRITTEN}],
        }))
        .unwrap();
        assert_eq!(from_text, from_json);
        assert_eq!(
            from_text.to_value().unwrap()["args"][1]["timestamp"],
            WRITTEN
        );
        assert!(!from_text.is_valid());
    }
}
