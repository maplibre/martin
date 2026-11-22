//! CQL2 operator precedence, as used to decide grouping when rendering.
//!
//! [`Expr::to_text`] and the SQL backends both consult this to decide which operands need
//! parentheses. The cql2-text parser keeps its own ordering, expressed as the sequence of `Op`
//! registrations pest's Pratt parser requires; `parser_ordering_matches_this_table` checks the two
//! agree.
//!
//! The levels follow the CQL2 BNF: `booleanExpression` / `booleanTerm` / `booleanFactor` /
//! `booleanPrimary`, then `predicate`, then `numericExpression`. Comparison, `LIKE`, `BETWEEN`,
//! `IN` and `IS NULL` share the predicate level, which is enough to decide grouping; the parser
//! distinguishes further within it.
//!
//! [`Expr::to_text`]: crate::Expr::to_text

use crate::Expr;

/// Loosest.
pub(crate) const OR: u8 = 1;
pub(crate) const AND: u8 = 2;
pub(crate) const NOT: u8 = 3;
/// Comparison, `LIKE`, `BETWEEN`, `IN`, `IS NULL` — one non-associative level.
pub(crate) const PREDICATE: u8 = 4;
pub(crate) const ADDITIVE: u8 = 5;
pub(crate) const MULTIPLICATIVE: u8 = 6;
pub(crate) const POWER: u8 = 7;
/// Anything that renders as a self-delimiting token: literals, property names, function calls,
/// arrays, and already-parenthesized expressions. Never needs wrapping.
pub(crate) const ATOM: u8 = u8::MAX;

/// The precedence of a CQL2 operator name.
///
/// What matters here is the shape an operator *renders* in, not the level the grammar parses it at.
/// Anything unrecognized is a function call (`casei(...)`, `s_intersects(...)`, ...), which renders
/// with its own parentheses in cql2-text and so binds as tightly as an atom.
///
/// `div` is one of those, despite being an arithmetic operator the grammar also accepts infix
/// (`Divide = { "/" | ^"div" }`): neither renderer emits it that way. `to_text` falls past its
/// arithmetic arm to the function-call fallback and the SQL backend has no `div` arm either, so both
/// write `div(a, b)`, whose own parentheses and commas delimit the operands.
///
/// The array operators are here for the mirror-image reason: `a_contains(x, y)` is a function call
/// in cql2-text, but renders as the infix `x @> y` in SQL. That stays unambiguous because `@>`, `<@`,
/// `@@` and `=` all bind more tightly than the boolean and `IS NULL` contexts a predicate can appear
/// in.
pub(crate) fn of_op(op: &str) -> u8 {
    match op.to_lowercase().as_str() {
        "or" => OR,
        "and" => AND,
        "not" => NOT,
        "=" | "<>" | "!=" | "<" | ">" | "<=" | ">=" | "like" | "between" | "in" | "isnull" => {
            PREDICATE
        }
        "+" | "-" => ADDITIVE,
        "*" | "/" | "%" => MULTIPLICATIVE,
        "^" => POWER,
        _ => ATOM,
    }
}

/// The precedence of an expression, i.e. of its outermost operator.
pub(crate) fn of_expr(expr: &Expr) -> u8 {
    match expr {
        Expr::Operation { op, .. } => of_op(op),
        // Literals, properties, arrays, geometries, `DATE(..)`, `INTERVAL(..)`, `BBOX(..)` all
        // render as self-delimiting tokens.
        _ => ATOM,
    }
}

/// What an operator demands of its operands before they can be printed without parentheses.
///
/// `first` applies to the leftmost operand and `rest` to the others. For a left-associative
/// operator the two differ by one, so `a - (b - c)` keeps its parentheses while `a - b - c` stays
/// flat. Associative and non-associative operators set them equal.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Operands {
    pub(crate) first: u8,
    pub(crate) rest: u8,
}

impl Operands {
    /// Whether the operand at `index` has to be parenthesized, i.e. whether it binds more loosely
    /// than this operator demands in that position.
    pub(crate) fn needs_parens(&self, index: usize, operand: &Expr) -> bool {
        let required = if index == 0 { self.first } else { self.rest };
        of_expr(operand) < required
    }
}

/// The operand requirements of a CQL2 operator, derived from its own precedence.
///
/// Three shapes cover every operator:
///
/// - **Associative** (`and`, `or`): an operand of equal precedence needs no parentheses on either
///   side, so a chain prints flat.
/// - **Left-associative** (arithmetic): only the right operand must bind more tightly, so
///   `a - b - c` prints flat while `a - (b - c)` keeps its parentheses.
/// - **Non-associative** (the predicates, and prefix `not`): every operand must bind more tightly,
///   so a predicate under a predicate is always parenthesized.
///
/// A function call delimits its own arguments, so it demands nothing of them.
pub(crate) fn operands(op: &str) -> Operands {
    let precedence = of_op(op);
    match precedence {
        ATOM => Operands { first: 0, rest: 0 },
        // `NOT` joins the associative pair: `NOT a = 1` already means `NOT (a = 1)`, since a
        // predicate binds tighter.
        AND | OR | NOT => Operands {
            first: precedence,
            rest: precedence,
        },
        ADDITIVE | MULTIPLICATIVE | POWER => Operands {
            first: precedence,
            rest: precedence + 1,
        },
        _ => Operands {
            first: precedence + 1,
            rest: precedence + 1,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The ladder must stay strictly increasing for `needs_parens` to mean anything. Bound to a
    /// local first, since clippy rejects asserting on constants directly.
    #[test]
    fn levels_are_strictly_increasing() {
        let increasing = OR < AND
            && AND < NOT
            && NOT < PREDICATE
            && PREDICATE < ADDITIVE
            && ADDITIVE < MULTIPLICATIVE
            && MULTIPLICATIVE < POWER
            && POWER < ATOM;
        assert!(increasing);
    }

    /// The parser's Pratt ordering and this table are written separately, so they are checked
    /// against each other behaviourally: for each pair, the tighter operator must end up nested
    /// inside the looser one.
    #[test]
    fn parser_ordering_matches_this_table() {
        const BY_LEVEL: &[(&str, &str)] = &[
            ("or", "and"),
            ("and", "="),
            ("=", "+"),
            ("+", "*"),
            ("*", "^"),
        ];
        for (looser, tighter) in BY_LEVEL {
            assert!(
                of_op(looser) < of_op(tighter),
                "{looser} should bind more loosely than {tighter}"
            );
            let source = format!("a {looser} b {tighter} c");
            let Ok(parsed) = source.parse::<Expr>() else {
                panic!("{source} should parse");
            };
            let Expr::Operation { op, .. } = &parsed else {
                panic!("{source} should parse to an operation, got {parsed:?}");
            };
            assert_eq!(
                op, looser,
                "{source} should be rooted at {looser}, the looser operator"
            );
        }
    }

    #[test]
    fn operator_names_are_case_insensitive() {
        assert_eq!(of_op("AND"), of_op("and"));
        assert_eq!(of_op("isNull"), of_op("isnull"));
        assert_eq!(of_op("Like"), of_op("like"));
    }

    #[test]
    fn unknown_operators_are_function_calls() {
        assert_eq!(of_op("casei"), ATOM);
        assert_eq!(of_op("s_intersects"), ATOM);
        // ...and a function call never parenthesizes its arguments.
        assert!(!operands("casei").needs_parens(0, &Expr::Bool(true)));
    }

    /// `div` is written infix by the grammar but rendered as a call by both backends, and it is the
    /// rendering that decides grouping.
    #[test]
    fn div_is_rendered_as_a_call() {
        assert_eq!(of_op("div"), ATOM);
        assert_eq!(of_op("DIV"), ATOM);
        let or: Expr = "a or b".parse().unwrap();
        assert!(!operands("div").needs_parens(0, &or));
        // And a `div` is an atom wherever it appears, so nothing wraps it either.
        let quotient: Expr = "a div b".parse().unwrap();
        assert!(!operands("*").needs_parens(1, &quotient));
    }

    #[test]
    fn associative_operators_stay_flat() {
        let and: Expr = "a and b".parse().unwrap();
        // An `and` nested under an `and` on either side needs no parentheses.
        assert!(!operands("and").needs_parens(0, &and));
        assert!(!operands("and").needs_parens(1, &and));
    }

    #[test]
    fn left_associative_arithmetic_wraps_only_on_the_right() {
        let sub: Expr = "b - c".parse().unwrap();
        // `a - b - c` stays flat, but `a - (b - c)` keeps its parentheses.
        assert!(!operands("-").needs_parens(0, &sub));
        assert!(operands("-").needs_parens(1, &sub));
    }

    #[test]
    fn looser_children_are_wrapped() {
        let or: Expr = "a or b".parse().unwrap();
        assert!(operands("and").needs_parens(1, &or));
        assert!(operands("=").needs_parens(0, &or));
        assert!(operands("*").needs_parens(1, &or));
    }

    #[test]
    fn tighter_children_are_not_wrapped() {
        let and: Expr = "a and b".parse().unwrap();
        let product: Expr = "b * c".parse().unwrap();
        assert!(!operands("or").needs_parens(0, &and));
        assert!(!operands("+").needs_parens(0, &product));
    }
}
