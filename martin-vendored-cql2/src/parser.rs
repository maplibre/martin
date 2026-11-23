use crate::{Error, Expr, Geometry};
use pest::{iterators::Pairs, pratt_parser::PrattParser, Parser};
use std::borrow::Cow;

/// Parses a cql2-text string into a CQL2 expression.
///
/// # Examples
///
/// ```
/// let s = "landsat:scene_id = 'LC82030282019133LGN00'";
/// let expr = cql2::parse_text(s);
/// ```
pub fn parse_text(s: &str) -> Result<Expr, Error> {
    // `ExprRoot` is anchored between `SOI` and `EOI`, so input the grammar cannot consume in full
    // is a parse error rather than a silently truncated expression.
    let mut pairs = CQL2Parser::parse(Rule::ExprRoot, s).map_err(Box::new)?;
    let expr = pairs
        .next()
        .ok_or_else(|| Error::InvalidCql2Text(s.to_string()))?;
    parse_expr(expr.into_inner()).map(crate::expr::normalize)
}

#[derive(pest_derive::Parser)]
#[grammar = "cql2.pest"]
struct CQL2Parser;

lazy_static::lazy_static! {
    static ref PRATT_PARSER: PrattParser<Rule> = {
        use pest::pratt_parser::{Assoc::*, Op};
        use Rule::*;
        PrattParser::new()
            // Ordered loosest to tightest, mirroring `crate::precedence`.
            .op(Op::infix(Or, Left))
            .op(Op::infix(And, Left))
            .op(Op::prefix(UnaryNot))
            .op(Op::infix(Eq, Right))
            .op(
                Op::infix(NotEq, Right) |
                Op::infix(Gt, Right) |
                Op::infix(GtEq, Right) |
                Op::infix(Lt, Right) |
                Op::infix(LtEq, Right)
            )
            .op(Op::infix(Like, Right))
            .op(Op::infix(In, Left))
            // `BETWEEN` is a postfix predicate on its left operand, binding looser than arithmetic
            // (`a + b BETWEEN 1 AND 2` brackets the sum) and tighter than the boolean connectives.
            .op(Op::postfix(IsNullPostfix) | Op::postfix(BetweenPostfix))
            .op(
                Op::infix(Add, Left) |
                Op::infix(Subtract, Left)
            )
            .op(
                Op::infix(Multiply, Left) |
                Op::infix(Divide, Left) |
                Op::infix(Modulo, Left)
            )
            .op(Op::infix(Power, Left))
            .op(Op::prefix(Negative))
        };
}

/// Unwraps a quoted token, undoing the doubling that escapes the quote character inside it.
pub(crate) fn strip_quotes(s: &str) -> Cow<'_, str> {
    for quote in ['"', '\''] {
        if let Some(inner) = s
            .strip_prefix(quote)
            .and_then(|rest| rest.strip_suffix(quote))
        {
            let doubled = [quote, quote].iter().collect::<String>();
            return if inner.contains(&doubled) {
                Cow::Owned(inner.replace(&doubled, &quote.to_string()))
            } else {
                Cow::Borrowed(inner)
            };
        }
    }
    Cow::Borrowed(s)
}

/// Replaces every internal run of whitespace with a single space, and removes leading and trailing
/// runs entirely.
fn collapse_whitespace(wkt: &str) -> String {
    wkt.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Restores a one-element array operand that the grammar read as a parenthesized scalar.
///
/// `(1)` is ambiguous in cql2-text: `AtomicExpr` tries grouping before an array literal, so a
/// single-element list arrives as the value itself. Only the operators whose operand is a list are
/// affected, and only a bare scalar is rewrapped — a property or an expression may legitimately
/// evaluate to an array.
fn restore_single_element_array(op: &str, args: &mut [Box<Expr>]) {
    if !crate::expr::ARRAYOPS
        .iter()
        .any(|name| name.eq_ignore_ascii_case(op))
    {
        return;
    }
    for arg in args.iter_mut() {
        if matches!(
            arg.as_ref(),
            Expr::Float(_) | Expr::Literal(_) | Expr::Bool(_)
        ) {
            **arg = Expr::Array(vec![arg.clone()]);
        }
    }
}

fn parse_expr(expression_pairs: Pairs<'_, Rule>) -> Result<Expr, Error> {
    PRATT_PARSER
        .map_primary(|primary| match primary.as_rule() {
            Rule::Expr | Rule::ExpressionInParentheses | Rule::BetweenOperand => {
                parse_expr(primary.into_inner())
            }
            Rule::DECIMAL | Rule::Double => Ok(Expr::Float(primary.as_str().parse::<f64>()?)),
            Rule::SingleQuotedString => {
                Ok(Expr::Literal(strip_quotes(primary.as_str()).to_string()))
            }
            Rule::True | Rule::False => Ok(Expr::Bool(primary.as_rule() == Rule::True)),
            Rule::Identifier => Ok(Expr::Property {
                property: strip_quotes(primary.as_str()).to_string(),
            }),
            Rule::GEOMETRY => {
                // CQL2 allows coordinates past the second without the `Z` or `ZM` marker OGC WKT
                // requires, so the marker is spliced in before the geometry is handed on. Which one
                // follows from the widest coordinate the grammar matched.
                let start = primary.as_span().start();
                let s = primary.as_str().to_string();
                let pairs = primary.into_inner();
                // The grammar matches a nested collection so that it is read as the geometry it
                // looks like rather than as a function call, and it is refused here: CQL2 gives a
                // collection's members as the six non-collection types, so there is no cql2-json
                // encoding for one. More than one `GEOMETRYCOLLECTION` in the token is nesting,
                // since a collection is the only rule a second one can appear inside.
                if pairs
                    .clone()
                    .flatten()
                    .filter(|pair| pair.as_rule() == Rule::GEOMETRY_COLLECTION)
                    .count()
                    > 1
                {
                    return Err(Error::NestedGeometryCollection);
                }
                let marker = if pairs.find_first_tagged("four_d").is_some() {
                    " ZM"
                } else if pairs.find_first_tagged("three_d").is_some() {
                    " Z"
                } else {
                    return Ok(Expr::Geometry(Geometry::Wkt(collapse_whitespace(&s))));
                };
                // Every unmarked geometry in the token needs it. A collection carries the marker and
                // so does each of its members, and no WKT reader accepts a mix.
                let mut slots: Vec<(usize, usize)> = pairs
                    .flatten()
                    .filter(|pair| matches!(pair.as_rule(), Rule::ZM))
                    .filter(|pair| pair.as_str().chars().all(char::is_whitespace))
                    .map(|pair| (pair.as_span().start() - start, pair.as_span().end() - start))
                    .collect();
                slots.sort_unstable();
                // Back to front, so the offsets ahead of each splice stay valid.
                let tagged = slots.into_iter().rev().fold(s, |acc, (lo, hi)| {
                    format!("{}{marker}{}", &acc[..lo], &acc[hi..])
                });
                Ok(Expr::Geometry(Geometry::Wkt(collapse_whitespace(&tagged))))
            }
            Rule::Function => {
                let mut pairs = primary.into_inner();
                // cql2-text is case-insensitive for operator names, but a user-supplied function
                // name keeps the case the author wrote.
                let op = crate::expr::canonical_op(&strip_quotes(
                    pairs
                        .next()
                        .expect("the grammar guarantees that there is always an op")
                        .as_str(),
                ));
                let mut args = Vec::new();
                for pair in pairs {
                    args.push(Box::new(parse_expr(pair.into_inner())?))
                }
                restore_single_element_array(&op, &mut args);
                match op.to_lowercase().as_str() {
                    "interval" => Ok(Expr::Interval { interval: args }),
                    "date" => Ok(Expr::Date {
                        date: args
                            .into_iter()
                            .next()
                            .ok_or(Error::MissingArgument("date"))?,
                    }),
                    "timestamp" => Ok(Expr::Timestamp {
                        timestamp: args
                            .into_iter()
                            .next()
                            .ok_or(Error::MissingArgument("timestamp"))?,
                    }),
                    "bbox" => Ok(Expr::BBox { bbox: args }),
                    // The function-call spelling `in(a, 1, 2)` arrives as a flat argument list, but
                    // `in` is always `[value, array]`, per the JSON schema's `inListOperands`.
                    "in" => {
                        let mut args = args.into_iter();
                        let value = args.next().ok_or(Error::MissingArgument("in"))?;
                        let list = match args.len() {
                            1 => match *args.next().expect("length checked above") {
                                array @ Expr::Array(_) => array,
                                other => Expr::Array(vec![Box::new(other)]),
                            },
                            _ => Expr::Array(args.collect()),
                        };
                        Ok(Expr::Operation {
                            op,
                            args: vec![value, Box::new(list)],
                        })
                    }
                    _ => Ok(Expr::Operation { op, args }),
                }
            }
            Rule::Array => {
                let pairs = primary.into_inner();
                let mut array_elements = Vec::new();
                for pair in pairs {
                    array_elements.push(Box::new(parse_expr(pair.into_inner())?))
                }
                Ok(Expr::Array(array_elements))
            }
            Rule::Null => Ok(Expr::Null),

            rule => unreachable!("parse_expr expected atomic rule, found {:?}", rule),
        })
        .map_infix(|lhs, op, rhs| {
            let lhs = lhs?;
            let rhs = rhs?;

            // `LIKE` and `IN` carry an optional leading `NOT` as a `NotFlag` child, so the matched
            // text spans both words. Take the name from the rule and the negation from the child,
            // which keeps both independent of the whitespace between them.
            let notflag = op
                .clone()
                .into_inner()
                .next()
                .is_some_and(|pair| pair.as_rule() == Rule::NotFlag);
            let opstring = match op.as_rule() {
                Rule::Like => "like".to_string(),
                Rule::In => "in".to_string(),
                _ => op.as_str().to_lowercase(),
            };

            // `(1)` parses as a parenthesized scalar, since the grammar tries grouping before an
            // array literal. `in`, the one infix operator whose right operand is a list, restores it.
            let rhs = if opstring == "in" && !matches!(rhs, Expr::Array(_)) {
                Expr::Array(vec![Box::new(rhs)])
            } else {
                rhs
            };

            let retexpr = Expr::Operation {
                op: opstring,
                args: vec![Box::new(lhs), Box::new(rhs)],
            };

            if notflag {
                return Ok(Expr::Operation {
                    op: "not".to_string(),
                    args: vec![Box::new(retexpr)],
                });
            }
            Ok(retexpr)
        })
        .map_prefix(|op, child| {
            let child = child?;
            match op.as_rule() {
                Rule::UnaryNot => Ok(Expr::Operation {
                    op: "not".to_string(),
                    args: vec![Box::new(child)],
                }),
                Rule::Negative => match child {
                    // A negated numeric literal is itself a numeric literal, e.g.
                    // `-2` is `Float(-2.0)`, not `-1 * 2`.
                    Expr::Float(v) => Ok(Expr::Float(-v)),
                    _ => Ok(Expr::Operation {
                        op: "*".to_string(),
                        args: vec![Box::new(Expr::Float(-1.0)), Box::new(child)],
                    }),
                },
                rule => unreachable!("parse_expr expected prefix operator, found {:?}", rule),
            }
        })
        .map_postfix(|child, op| {
            let child = child?;
            let rule = op.as_rule();
            let mut inner = op.into_inner();

            // Both postfix predicates carry an optional `NOT` as their first child.
            let mut notflag = false;
            if inner.peek().map(|pair| pair.as_rule()) == Some(Rule::NotFlag) {
                let _ = inner.next();
                notflag = true;
            }

            let retexpr = match rule {
                Rule::IsNullPostfix => Expr::Operation {
                    op: "isNull".to_string(),
                    args: vec![Box::new(child)],
                },
                Rule::BetweenPostfix => {
                    let mut bounds = Vec::with_capacity(2);
                    for bound in inner {
                        bounds.push(Box::new(parse_expr(bound.into_inner())?));
                    }
                    let [low, high]: [Box<Expr>; 2] = bounds
                        .try_into()
                        .map_err(|_| Error::MissingArgument("between"))?;
                    Expr::Operation {
                        op: "between".to_string(),
                        args: vec![Box::new(child), low, high],
                    }
                }
                rule => unreachable!("parse_expr expected postfix operator, found {:?}", rule),
            };
            if notflag {
                return Ok(Expr::Operation {
                    op: "not".to_string(),
                    args: vec![Box::new(retexpr)],
                });
            };
            Ok(retexpr)
        })
        .parse(expression_pairs)
}

#[cfg(test)]
mod tests {
    use super::{CQL2Parser, Rule};
    use crate::Expr;
    use pest::Parser;

    #[test]
    fn point_zm() {
        let _ = CQL2Parser::parse(Rule::GEOMETRY, "POINT ZM(-105.1019 40.1672 4981 42)").unwrap();
    }

    /// A four-ordinate coordinate written without a marker is tagged `ZM`, as a three-ordinate one
    /// is tagged `Z`. Untagged, the rendering is text no WKT reader accepts.
    #[test]
    fn four_dimensional_coordinates_are_tagged() {
        for source in [
            "s_intersects(geom, POINT(1 2 3 4))",
            "s_intersects(geom, POINT ZM(1 2 3 4))",
        ] {
            let text = super::parse_text(source)
                .unwrap()
                .to_text()
                .expect("renders as text");
            assert_eq!(text, "s_intersects(geom, POINT ZM(1 2 3 4))");
        }
    }

    #[test]
    fn bbox() {
        let bbox: Expr =
            super::parse_text("bbox(9.978199, 53.541309, 10.010294, 53.557241)").unwrap();
        assert!(matches!(bbox, Expr::BBox { .. }));
    }
}
