use crate::{geometry::spatial_op, precedence, temporal::temporal_op, Error, Geometry, Validator};
use geo_types::{coord, Geometry as GGeom, Rect};
use json_dotpath::DotPaths;
use like::Like;
use pg_escape::quote_identifier;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::HashSet, fmt::Debug, ops::Add, str::FromStr, sync::OnceLock};
use unaccent::unaccent;
use wkt::TryFromWkt;

/// Boolean Operators
pub const BOOLOPS: &[&str] = &["and", "or"];

/// Equality Operators
pub const EQOPS: &[&str] = &["=", "<>"];

/// Comparison Operators
pub const CMPOPS: &[&str] = &[">", ">=", "<", "<="];

/// Spatial Operators
pub const SPATIALOPS: &[&str] = &[
    "s_equals",
    "s_intersects",
    "s_disjoint",
    "s_touches",
    "s_within",
    "s_overlaps",
    "s_crosses",
    "s_contains",
];

/// Temporal Operators
pub const TEMPORALOPS: &[&str] = &[
    "t_before",
    "t_after",
    "t_meets",
    "t_metBy",
    "t_overlaps",
    "t_overlappedBy",
    "t_starts",
    "t_startedBy",
    "t_during",
    "t_contains",
    "t_finishes",
    "t_finishedBy",
    "t_equals",
    "t_disjoint",
    "t_intersects",
];

/// Arithmetic Operators
pub const ARITHOPS: &[&str] = &["+", "-", "*", "/", "%", "^", "div"];

/// Array Operators
pub const ARRAYOPS: &[&str] = &["a_equals", "a_contains", "a_containedBy", "a_overlaps"];

/// The arithmetic operators that take any number of operands, chained to the left.
///
/// `{"op": "+", "args": [a, b, c]}` renders as `a + b + c` in cql2-text and in SQL alike, which both
/// read as `(a + b) + c`. The other two arithmetic operators are binary: `^` renders as `power(a, b)`
/// in SQL and requires exactly two operands in cql2-text, and `div` is a function call in both.
const CHAINED_ARITHOPS: &[&str] = &["+", "-", "*", "/", "%"];

/// Operator names that belong to none of the categories above.
const OTHER_OPS: &[&str] = &["not", "like", "between", "in", "isNull", "casei", "accenti"];

/// Every operator name the JSON schema defines, in the spelling it defines.
///
/// Assembled from the category constants so each name is written once; `canonical_ops_match_the_schema`
/// pins the result against the schema itself.
fn canonical_ops() -> impl Iterator<Item = &'static str> {
    [
        BOOLOPS,
        EQOPS,
        CMPOPS,
        SPATIALOPS,
        TEMPORALOPS,
        ARITHOPS,
        ARRAYOPS,
        OTHER_OPS,
    ]
    .into_iter()
    .flat_map(|ops| ops.iter().copied())
}

/// Puts an expression into the crate's canonical form.
///
/// Both encodings run through this, so cql2-text and cql2-json describe the same expression
/// identically:
///
/// - `and` and `or` are associative, so a chain becomes one n-ary operation. A chain also has to be
///   flat to survive a text round trip, since the renderers omit the parentheses that would
///   otherwise be the only record of the nesting.
/// - A timestamp denotes an instant, so each instant has one spelling.
/// - Every operator name is resolved to its canonical spelling, which is what makes the
///   case-sensitive spellings the cql2-json schema requires come out right.
pub(crate) fn normalize(expr: Expr) -> Expr {
    match expr {
        Expr::Operation { op, args } => {
            let op = canonical_op(&op);
            let args = args.into_iter().map(|arg| Box::new(normalize(*arg)));
            if op == "and" || op == "or" {
                let mut flat: Vec<Box<Expr>> = Vec::new();
                for arg in args {
                    match *arg {
                        Expr::Operation {
                            op: nested,
                            args: inner,
                        } if nested == op => flat.extend(inner),
                        other => flat.push(Box::new(other)),
                    }
                }
                Expr::Operation { op, args: flat }
            } else {
                Expr::Operation {
                    op,
                    args: args.collect(),
                }
            }
        }
        Expr::Array(items) => {
            Expr::Array(items.into_iter().map(|i| Box::new(normalize(*i))).collect())
        }
        Expr::Timestamp { timestamp } => Expr::Timestamp {
            timestamp: Box::new(normalize_instant(*timestamp)),
        },
        Expr::Interval { interval } => Expr::Interval {
            interval: interval
                .into_iter()
                .map(|bound| Box::new(normalize_instant(*bound)))
                .collect(),
        },
        other => other,
    }
}

fn normalize_instant(expr: Expr) -> Expr {
    match expr {
        Expr::Literal(value) => Expr::Literal(crate::temporal::canonical_timestamp(&value)),
        other => normalize(other),
    }
}

/// Renders a string as a cql2-text literal: single-quoted, with an embedded quote doubled.
fn literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

/// Names the cql2-text grammar reads as something other than an identifier.
///
/// `Literal` is tried before `Identifier`, so a bare `true`, `false` or `null` comes back as that
/// value rather than as a name, and `not` is consumed as the prefix operator, leaving nothing for
/// the expression that follows. Every other keyword — `and`, `is`, `like`, `between`, `div` — reads
/// as an identifier where one is expected, so only these four need the quotes.
const GRAMMAR_RESERVED: &[&str] = &["true", "false", "null", "not"];

/// Renders a name as a cql2-text identifier, quoting it only where the grammar requires.
///
/// [`quote_identifier`] applies PostgreSQL's rules, which quote any name that is not lowercase, and
/// every SQL keyword besides. A cql2-text identifier is case-sensitive and admits `_`, `.` and `:`,
/// so names like `t_metBy`, `Foo` and `landsat:scene_id` are written bare. This is what both a
/// property and a function name are rendered with, so one name has one spelling wherever it appears.
fn identifier(name: &str) -> String {
    let mut chars = name.chars();
    let is_bare = chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | ':'))
        && !GRAMMAR_RESERVED
            .iter()
            .any(|reserved| reserved.eq_ignore_ascii_case(name));
    if is_bare {
        name.to_string()
    } else {
        quote_identifier(name).to_string()
    }
}

/// Reads a bare literal as the temporal value it spells.
///
/// A value with a time component names an instant; a plain calendar date names a day.
fn temporal_literal(value: &str) -> Expr {
    let literal = Box::new(Expr::Literal(value.to_string()));
    if value.contains('T') || value.contains(' ') {
        Expr::Timestamp { timestamp: literal }
    } else {
        Expr::Date { date: literal }
    }
}

/// Whether an operand denotes a region a spatial predicate can be evaluated against.
fn is_region(expr: &Expr) -> bool {
    matches!(expr, Expr::Geometry(_) | Expr::BBox { .. })
}

/// The number of operands an operator's reduction indexes, if it is fixed.
///
/// `None` means the reduction reads its operands as a list and accepts any number.
fn reduce_arity(op: &str) -> Option<usize> {
    match op {
        "isNull" | "not" | "casei" | "accenti" => Some(1),
        "between" => Some(3),
        _ => None,
    }
}

/// Resolves an operator name to its canonical CQL2 spelling.
///
/// Operator names are case-insensitive, so `T_METBY` and `t_metBy` name the same operator, and the
/// grammar accepts one spelling CQL2 writes differently. Anything CQL2 does not define is a
/// user-supplied function name, returned unchanged because its case is the author's to choose.
pub(crate) fn canonical_op(name: &str) -> String {
    let aliased = ALIASES
        .iter()
        .find(|(alias, _)| alias.eq_ignore_ascii_case(name))
        .map_or(name, |(_, canonical)| canonical);
    canonical_ops()
        .find(|canonical| canonical.eq_ignore_ascii_case(aliased))
        .map_or_else(|| aliased.to_string(), str::to_string)
}

/// Alternate spellings of operators CQL2 does define, resolved case-insensitively.
///
/// Resolved here rather than in one backend, so that a single fold serves the evaluator, both
/// renderers and both encodings: `ST_Intersects(a, b)` is the same expression as
/// `s_intersects(a, b)` whichever of them reads it.
///
/// `eq`, `lt`, `ne` and friends are deliberately absent: the schema defines no such operators, so
/// they can only arrive as user-defined function names and must be left alone. `div` is likewise not
/// aliased to `/` — CQL2 defines it as integer division, a distinct operator.
const ALIASES: &[(&str, &str)] = &[
    // A spelling the cql2-text grammar itself accepts: `NotEq = { "<>" | "!=" }`.
    ("!=", "<>"),
    // How SQL and PostGIS spell the spatial predicates. One per `SPATIALOPS` entry, which
    // `every_spatial_operator_has_an_st_alias` pins.
    ("st_equals", "s_equals"),
    ("st_intersects", "s_intersects"),
    ("st_disjoint", "s_disjoint"),
    ("st_touches", "s_touches"),
    ("st_within", "s_within"),
    ("st_overlaps", "s_overlaps"),
    ("st_crosses", "s_crosses"),
    ("st_contains", "s_contains"),
    // How the earlier drafts of CQL2 spelled the two `intersects` predicates.
    ("intersects", "s_intersects"),
    ("anyinteracts", "t_intersects"),
];

/// A CQL2 expression.
///
/// # Examples
///
/// [Expr] implements [FromStr]:
///
/// ```
/// use cql2::Expr;
///
/// let expr: Expr = "landsat:scene_id = 'LC82030282019133LGN00'".parse().unwrap();
/// ```
///
/// Use [Expr::to_text], [Expr::to_json], and [crate::ToSqlAst::to_sql] to use the CQL2,
/// and use [Expr::is_valid] to check validity.
///
/// Deserializing normalizes, so every serde entry point agrees with [crate::parse_json].
// `remote = "Self"` turns the derives into inherent `Expr::serialize` and `Expr::deserialize`
// functions instead of trait impls, so the hand-written impls below can call them. Without the
// normalizing impl, `serde_json::from_str::<Expr>`, a `#[derive(Deserialize)]` struct holding an
// `Expr` field, and the bindings' mapping constructors would each produce an expression
// `parse_json` would have canonicalized, and two spellings of one filter would compare unequal.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, PartialOrd)]
#[serde(untagged, remote = "Self")]
#[allow(missing_docs)]
pub enum Expr {
    Operation { op: String, args: Vec<Box<Expr>> },
    Interval { interval: Vec<Box<Expr>> },
    Timestamp { timestamp: Box<Expr> },
    Date { date: Box<Expr> },
    Property { property: String },
    BBox { bbox: Vec<Box<Expr>> },
    Float(f64),
    Literal(String),
    Bool(bool),
    Array(Vec<Box<Expr>>),
    Geometry(Geometry),
    Null,
}

impl Serialize for Expr {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        // The inherent function the `remote = "Self"` derive generated, not this method.
        Expr::serialize(self, serializer)
    }
}

impl<'de> Deserialize<'de> for Expr {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        // As above: the derived inherent function. Nested expressions come back through this impl,
        // so they are already normalized; `normalize` is idempotent, so the outer pass is free to
        // run over them again.
        Expr::deserialize(deserializer).map(normalize)
    }
}

impl TryFrom<Value> for Expr {
    type Error = Error;
    fn try_from(v: Value) -> Result<Expr, Error> {
        serde_json::from_value(v).map_err(Error::from)
    }
}
impl TryFrom<Expr> for Value {
    type Error = Error;
    fn try_from(v: Expr) -> Result<Value, Error> {
        serde_json::to_value(v).map_err(Error::from)
    }
}
impl TryFrom<Expr> for f64 {
    type Error = Error;
    fn try_from(v: Expr) -> Result<f64, Error> {
        match v {
            Expr::Float(v) => Ok(v),
            Expr::Literal(v) => f64::from_str(&v).map_err(Error::from),
            _ => Err(Error::ExprToF64(v)),
        }
    }
}

impl TryFrom<&Expr> for bool {
    type Error = Error;
    fn try_from(v: &Expr) -> Result<bool, Error> {
        match v {
            Expr::Bool(v) => Ok(*v),
            Expr::Literal(v) => bool::from_str(v).map_err(Error::from),
            _ => Err(Error::ExprToBool(v.clone())),
        }
    }
}

impl TryFrom<Expr> for String {
    type Error = Error;
    fn try_from(v: Expr) -> Result<String, Error> {
        match v {
            Expr::Literal(v) => Ok(v),
            Expr::Bool(v) => Ok(v.to_string()),
            Expr::Float(v) => Ok(v.to_string()),
            _ => Err(Error::ExprToBool(v)),
        }
    }
}

impl TryFrom<Expr> for GGeom {
    type Error = Error;
    fn try_from(v: Expr) -> Result<GGeom, Error> {
        match v {
            Expr::Geometry(ref g) => {
                GGeom::try_from_wkt_str(&g.to_wkt()?).map_err(|_| Error::ExprToGeom(v.clone()))
            }
            Expr::BBox { ref bbox } => {
                let [minx, miny, maxx, maxy] = match bbox.as_slice() {
                    [minx, miny, maxx, maxy] => [minx, miny, maxx, maxy],
                    [minx, miny, _minz, maxx, maxy, _maxz] => [minx, miny, maxx, maxy],
                    _ => return Err(Error::ExprToGeom(v.clone())),
                };
                let minx: f64 = minx.as_ref().clone().try_into()?;
                let miny: f64 = miny.as_ref().clone().try_into()?;
                let maxx: f64 = maxx.as_ref().clone().try_into()?;
                let maxy: f64 = maxy.as_ref().clone().try_into()?;
                let rec = Rect::new(coord! {x:minx, y:miny}, coord! {x:maxx,y:maxy});
                Ok(rec.into())
            }
            _ => Err(Error::ExprToGeom(v)),
        }
    }
}

impl TryFrom<Expr> for HashSet<String> {
    type Error = Error;
    fn try_from(v: Expr) -> Result<HashSet<String>, Error> {
        match v {
            Expr::Array(v) => {
                let mut h = HashSet::new();
                for el in v {
                    let _ = h.insert(el.to_text()?);
                }
                Ok(h)
            }
            _ => Err(Error::ExprToGeom(v)),
        }
    }
}

fn cmp_op<T: PartialEq + PartialOrd>(left: T, right: T, op: &str) -> Result<Expr, Error> {
    let out = match op {
        "=" => left == right,
        "<=" => left <= right,
        "<" => left < right,
        ">=" => left >= right,
        ">" => left > right,
        "<>" => left != right,
        _ => return Err(Error::OperationError()),
    };
    Ok(Expr::Bool(out))
}

fn arith_op(left: Expr, right: Expr, op: &str) -> Result<Expr, Error> {
    let left = f64::try_from(left)?;
    let right = f64::try_from(right)?;
    let out = match op {
        "+" => left + right,
        "-" => left - right,
        "*" => left * right,
        "/" => left / right,
        "%" => left % right,
        "^" => left.powf(right),
        // Integer division, which is what CQL2 defines `div` to be and what makes it a different
        // operator from `/`: `5 div 2` is 2, not 2.5.
        //
        // The quotient is truncated toward zero, so `-5 div 2` is -2. That is what Rust's `/` does
        // on integers, what PostgreSQL's `/` and `div()` do, and what the SQL standard requires;
        // flooring instead would make the answer depend on the sign of the operands.
        //
        // Dividing by zero has no integer answer — the IEEE infinity `/` yields is not one — so it
        // is reported as a failure, which leaves the operation unfolded exactly as an operand that
        // is not a number does.
        "div" => {
            if right == 0.0 {
                return Err(Error::OperationError());
            }
            (left / right).trunc()
        }
        _ => return Err(Error::OperationError()),
    };
    Ok(Expr::Float(out))
}

fn array_op(left: Expr, right: Expr, op: &str) -> Result<Expr, Error> {
    let left: HashSet<String> = left.try_into()?;
    let right: HashSet<String> = right.try_into()?;
    let out = match op {
        "a_equals" => left == right,
        "a_contains" => left.is_superset(&right),
        "a_containedBy" => left.is_subset(&right),
        "a_overlaps" => !left.is_disjoint(&right),
        _ => return Err(Error::OperationError()),
    };
    Ok(Expr::Bool(out))
}

/// Whether `op` names an operator CQL2 defines, rather than a user-supplied function.
///
/// A defined operator has semantics this crate knows, which is what lets NULL be propagated
/// through it. A function name is the author's, so what it does with a NULL argument is unknown
/// and the call is left for the caller to evaluate.
fn is_defined_operator(op: &str) -> bool {
    canonical_ops().any(|known| known == op)
}

/// Returns `true` if a *reduced* expression is still "unknown", i.e. its value
/// cannot be determined at reduction time.
///
/// This is the case for an unresolved property reference (a property that was
/// not found in the supplied JSON, or when no JSON was supplied) or an operation
/// that could not be folded to a concrete value. Predicates over unknown
/// operands must not be constant-folded, otherwise `reduce` would invent a
/// truth value for something it does not actually know.
///
/// [`Expr::Null`] is deliberately *not* unknown. It is a value like any other — the third truth
/// value of the three-valued logic CQL2 and SQL share — and an operation over it folds to what
/// that logic says it is, rather than being left for someone else to evaluate.
fn is_unknown(expr: &Expr) -> bool {
    match expr {
        Expr::Property { .. } | Expr::Operation { .. } => true,
        Expr::Interval { interval } => interval.iter().any(|e| is_unknown(e)),
        Expr::Date { date } => is_unknown(date),
        Expr::Timestamp { timestamp } => is_unknown(timestamp),
        Expr::Array(elements) => elements.iter().any(|e| is_unknown(e)),
        Expr::BBox { bbox } => bbox.iter().any(|e| is_unknown(e)),
        Expr::Float(_) | Expr::Literal(_) | Expr::Bool(_) | Expr::Geometry(_) | Expr::Null => false,
    }
}

impl Expr {
    /// Update this expression with values from the `properties` attribute of a JSON object
    ///
    ///  # Examples
    ///
    /// ```
    /// use serde_json::{json, Value};
    /// use cql2::Expr;
    /// use std::str::FromStr;
    ///
    /// let item = json!({"properties":{"eo:cloud_cover":10, "datetime": "2020-01-01 00:00:00Z", "boolfield": true}});
    ///
    /// let fromexpr: Expr = Expr::from_str("boolfield = true").unwrap();
    /// let reduced = fromexpr.reduce(Some(&item)).unwrap();
    /// let toexpr: Expr = Expr::from_str("true").unwrap();
    /// assert_eq!(reduced, toexpr);
    ///
    /// let fromexpr: Expr = Expr::from_str("\"eo:cloud_cover\" + 10").unwrap();
    /// let reduced = fromexpr.reduce(Some(&item)).unwrap();
    /// let toexpr: Expr = Expr::from_str("20").unwrap();
    /// assert_eq!(reduced, toexpr);
    ///
    /// let fromexpr: Expr = Expr::from_str("(bork=1) and (bork=1) and (bork=1 and true)").unwrap();
    /// let reduced = fromexpr.reduce(Some(&item)).unwrap();
    /// let toexpr: Expr = Expr::from_str("bork=1").unwrap();
    /// assert_eq!(reduced, toexpr);
    ///
    /// ```
    pub fn reduce(self, j: Option<&Value>) -> Result<Expr, Error> {
        match self {
            Expr::Property { ref property } => {
                let Some(j) = j else { return Ok(self) };
                if let Some(value) = j.dot_get::<Value>(property)? {
                    Expr::try_from(value)
                } else if let Some(value) = j.dot_get::<Value>(&format!("properties.{property}"))? {
                    Expr::try_from(value)
                } else {
                    Ok(self)
                }
            }
            Expr::Interval { ref interval } => {
                let [lo, hi] = interval.as_slice() else {
                    return Err(Error::InvalidNumberOfArguments {
                        name: "interval".to_string(),
                        actual: interval.len(),
                        expected: 2,
                    });
                };
                let start = lo.as_ref().clone().reduce(j)?;
                let end = hi.as_ref().clone().reduce(j)?;
                Ok(Expr::Interval {
                    interval: vec![Box::new(start), Box::new(end)],
                })
            }
            Expr::Operation { op, args } => {
                // Dispatch below matches canonical spellings, and an expression left unfolded keeps
                // the name it came in with.
                let op = canonical_op(&op);
                // Checked before any arm indexes into `args`, so a malformed expression is an
                // error rather than an abort. Operators reduced as whole lists take any arity.
                if let Some(expected) = reduce_arity(&op) {
                    if args.len() != expected {
                        return Err(Error::InvalidNumberOfArguments {
                            name: op,
                            actual: args.len(),
                            expected,
                        });
                    }
                }

                let args: Vec<Box<Expr>> = args
                    .into_iter()
                    .map(|expr| expr.reduce(j).map(Box::new))
                    .collect::<Result<_, _>>()?;

                if op == "isNull" {
                    if matches!(args[0].as_ref(), Expr::Null) {
                        Ok(Expr::Bool(true))
                    } else if is_unknown(args[0].as_ref()) {
                        if j.is_some() {
                            // We are reducing against a concrete record: an
                            // unresolved property means the field is absent (and
                            // therefore null) for this record, so IS NULL is true.
                            Ok(Expr::Bool(true))
                        } else {
                            // No data context: the value of the operand is unknown,
                            // so leave the predicate in place rather than folding it
                            // to a constant.
                            Ok(Expr::Operation {
                                op: "isNull".to_string(),
                                args,
                            })
                        }
                    } else {
                        Ok(Expr::Bool(false))
                    }
                } else if BOOLOPS.contains(&op.as_str()) {
                    let mut dedupargs: Vec<Box<Expr>> = vec![];
                    let mut nestedargs: Vec<Box<Expr>> = vec![];
                    for a in args {
                        match *a {
                            Expr::Operation {
                                op: nested,
                                args: inner,
                            } if nested == op => nestedargs.extend(inner),
                            _ => dedupargs.push(a),
                        }
                    }
                    dedupargs.append(&mut nestedargs);
                    // Operands are sorted so equal ones become adjacent. Not every pair is
                    // comparable — `Geometry` has no ordering — so incomparable operands fall back
                    // to a total order over their rendering, which keeps distinct operands distinct.
                    dedupargs.sort_by(|a, b| {
                        a.partial_cmp(b)
                            .unwrap_or_else(|| format!("{a:?}").cmp(&format!("{b:?}")))
                    });
                    dedupargs.dedup();

                    // The three truth values are counted apart from each other, and both apart from
                    // an operand that is not a truth value at all: NULL is a value the connectives
                    // define an answer for, an unfolded operand is one they cannot answer for.
                    let mut anytrue: bool = false;
                    let mut anyfalse: bool = false;
                    let mut anynull: bool = false;
                    let mut anyexp: bool = false;

                    for a in dedupargs.iter() {
                        if matches!(a.as_ref(), Expr::Null) {
                            anynull = true;
                            continue;
                        }
                        let b = bool::try_from(a.as_ref());
                        match b {
                            Ok(true) => {
                                anytrue = true;
                            }
                            Ok(false) => {
                                anyfalse = true;
                            }
                            _ => {
                                anyexp = true;
                            }
                        }
                    }
                    // One value of each connective absorbs every other operand, whatever it is:
                    // FALSE AND anything is FALSE and TRUE OR anything is TRUE, NULL and unfolded
                    // operands included. This is where three-valued logic differs most from
                    // propagating NULL blindly.
                    if op == "and" && anyfalse {
                        return Ok(Expr::Bool(false));
                    }
                    if op == "or" && anytrue {
                        return Ok(Expr::Bool(true));
                    }
                    // TRUE is the identity of AND, so a true operand says nothing about the answer
                    // and is dropped. (FALSE is the identity of OR, but a false operand is left in
                    // place there, which is the shape this crate has always emitted.)
                    if op == "and" && anytrue {
                        dedupargs.retain(|x| !bool::try_from(x.as_ref()).unwrap_or(false));
                    }
                    if dedupargs.len() == 1 {
                        Ok(*dedupargs.pop().unwrap())
                    } else if !anyexp && anynull {
                        // Nothing decides the answer and one operand is NULL, so the answer is
                        // NULL: `FALSE OR NULL` and `TRUE AND NULL` are both NULL.
                        Ok(Expr::Null)
                    } else if !anyexp && op == "or" {
                        // Every operand is FALSE.
                        Ok(Expr::Bool(false))
                    } else if !anyexp && op == "and" {
                        // Every operand was TRUE and has been dropped.
                        Ok(Expr::Bool(true))
                    } else {
                        Ok(Expr::Operation {
                            op,
                            args: dedupargs,
                        })
                    }
                } else if op == "not" {
                    match args[0].as_ref() {
                        Expr::Bool(v) => Ok(Expr::Bool(!v)),
                        // The negation of "unknown" is "unknown".
                        Expr::Null => Ok(Expr::Null),
                        _ => Ok(Expr::Operation { op, args }),
                    }
                } else if is_defined_operator(&op)
                    && args.iter().any(|arg| matches!(arg.as_ref(), Expr::Null))
                {
                    // Every operator CQL2 defines besides the connectives above is NULL-propagating,
                    // as it is in SQL: a comparison against NULL is NULL rather than false, and so is
                    // arithmetic, a spatial or temporal predicate, `LIKE`, `BETWEEN` and the rest.
                    // A user-supplied function is not folded, because what it makes of a NULL
                    // argument is not this crate's to decide.
                    Ok(Expr::Null)
                } else if op == "casei" {
                    match args[0].as_ref() {
                        Expr::Literal(v) => Ok(Expr::Literal(v.to_lowercase())),
                        _ => Ok(Expr::Operation { op, args }),
                    }
                } else if op == "accenti" {
                    match args[0].as_ref() {
                        Expr::Literal(v) => Ok(Expr::Literal(unaccent(v))),
                        _ => Ok(Expr::Operation { op, args }),
                    }
                } else if op == "between" {
                    if args.iter().any(|a| is_unknown(a)) {
                        // One of the operands is unknown, so we can't evaluate the
                        // range check; leave the predicate in place.
                        Ok(Expr::Operation { op, args })
                    } else {
                        Ok(Expr::Bool(args[0] >= args[1] && args[0] <= args[2]))
                    }
                } else if CHAINED_ARITHOPS.contains(&op.as_str()) && args.len() > 2 {
                    // Both renderings chain these, so the evaluator has to read the chain the same
                    // way they write it: `10 - 3 - 2` is `(10 - 3) - 2`, which is 5, not 9. The
                    // direction is the whole of the meaning for `-` and `/`.
                    //
                    // Each step is an ordinary two-operand reduction, so a chain folds exactly as
                    // the pairs it is made of would.
                    let mut operands = args.iter().map(|arg| arg.as_ref().clone());
                    let first = operands.next().expect("length checked above");
                    let folded = operands.try_fold(first, |left, right| {
                        Expr::Operation {
                            op: op.clone(),
                            args: vec![Box::new(left), Box::new(right)],
                        }
                        .reduce(j)
                    })?;
                    // A chain that did not fold all the way keeps the flat shape it came in with,
                    // rather than the half-folded nest the fold left behind.
                    Ok(match folded {
                        Expr::Operation { .. } => Expr::Operation { op, args },
                        value => value,
                    })
                } else if args.len() != 2 {
                    Ok(Expr::Operation { op, args })
                } else {
                    // Two-arg operations
                    let mut left = args[0].as_ref().clone();
                    let mut right = args[1].as_ref().clone();

                    // If either operand is unknown (an unresolved property or an
                    // expression that did not fold to a concrete value) we cannot
                    // evaluate the operation, so leave it in place rather than
                    // constant-folding it to an incorrect value.
                    if is_unknown(&left) || is_unknown(&right) {
                        return Ok(Expr::Operation { op, args });
                    }

                    let is_temporal_relation = TEMPORALOPS.contains(&op.as_str());
                    let is_spatial_relation = SPATIALOPS.contains(&op.as_str());
                    let is_comparison =
                        EQOPS.contains(&op.as_str()) || CMPOPS.contains(&op.as_str());

                    // A bare literal beside a temporal operand is read as temporal too. For a
                    // comparison it takes its neighbour's kind, so `ts = DATE('2020-01-02')` asks
                    // whether the two name the same day. The interval relations instead read the
                    // literal on its own terms: widening an instant to a whole day would change
                    // which intervals it meets, and the SQL backend cannot widen a column at all.
                    match (&left, &right) {
                        (Expr::Date { .. }, Expr::Literal(ref v)) if is_temporal_relation => {
                            right = temporal_literal(v);
                        }
                        (Expr::Date { .. }, Expr::Literal(ref v)) => {
                            right = Expr::Date {
                                date: Box::new(Expr::Literal(v.clone())),
                            };
                        }
                        (Expr::Timestamp { .. }, Expr::Literal(ref v)) => {
                            right = Expr::Timestamp {
                                timestamp: Box::new(Expr::Literal(v.clone())),
                            };
                        }
                        (Expr::Literal(ref v), Expr::Date { .. }) if is_temporal_relation => {
                            left = temporal_literal(v);
                        }
                        (Expr::Literal(ref v), Expr::Date { .. }) => {
                            left = Expr::Date {
                                date: Box::new(Expr::Literal(v.clone())),
                            };
                        }
                        (Expr::Literal(ref v), Expr::Timestamp { .. }) => {
                            left = Expr::Timestamp {
                                timestamp: Box::new(Expr::Literal(v.clone())),
                            };
                        }
                        _ => {}
                    }

                    if is_temporal_relation {
                        match temporal_op(left, right, &op) {
                            Ok(reduced) => Ok(reduced),
                            Err(_) => Ok(Expr::Operation { op, args }),
                        }
                    } else if matches!(left, Expr::Date { .. } | Expr::Timestamp { .. })
                        && matches!(right, Expr::Date { .. } | Expr::Timestamp { .. })
                        && is_comparison
                    {
                        // convert both operands to DateRange and compare using PartialOrd/PartialEq
                        let l_dr = crate::temporal::DateRange::try_from(left)?;
                        let r_dr = crate::temporal::DateRange::try_from(right)?;
                        cmp_op(l_dr, r_dr, &op)
                    // Operands normally have to be the same kind to fold. A spatial predicate is
                    // the exception: a geometry and a bounding box are both regions, and comparing
                    // one against the other is the ordinary case.
                    } else if std::mem::discriminant(&left) == std::mem::discriminant(&right)
                        || (is_spatial_relation && is_region(&left) && is_region(&right))
                    {
                        if is_spatial_relation {
                            Ok(spatial_op(left, right, &op)
                                .unwrap_or_else(|_| Expr::Operation { op, args }))
                        } else if ARITHOPS.contains(&op.as_str()) {
                            Ok(arith_op(left, right, &op)
                                .unwrap_or_else(|_| Expr::Operation { op, args }))
                        } else if is_comparison {
                            Ok(cmp_op(left, right, &op)
                                .unwrap_or_else(|_| Expr::Operation { op, args }))
                        } else if ARRAYOPS.contains(&op.as_str()) {
                            Ok(array_op(left, right, &op)
                                .unwrap_or_else(|_| Expr::Operation { op, args }))
                        } else if op == "like" {
                            let l: String = left.try_into()?;
                            let r: String = right.try_into()?;
                            let m: bool = Like::<true>::like(l.as_str(), r.as_str())?;
                            Ok(Expr::Bool(m))
                        } else {
                            Ok(Expr::Operation { op, args })
                        }
                    } else if op == "in" {
                        // `x IN (a, b)` is `x = a OR x = b`, so a NULL among the candidates follows
                        // the same three-valued logic: a match still decides the answer, and
                        // without one the NULL leaves it unknown rather than false.
                        let has_null = matches!(&right, Expr::Array(items)
                            if items.iter().any(|item| matches!(item.as_ref(), Expr::Null)));
                        let l: String = left.to_text()?;
                        let r: HashSet<String> = right.try_into()?;
                        let isin: bool = r.contains(&l);
                        Ok(match (isin, has_null) {
                            (true, _) => Expr::Bool(true),
                            (false, true) => Expr::Null,
                            (false, false) => Expr::Bool(false),
                        })
                    } else {
                        Ok(Expr::Operation { op, args })
                    }
                }
            }
            _ => Ok(self),
        }
    }

    /// Run CQL against a JSON Value
    ///
    ///  # Examples
    ///
    /// ```
    /// use serde_json::{json, Value};
    /// use cql2::Expr;
    /// let item = json!({"properties":{"eo:cloud_cover":10, "datetime": "2020-01-01 00:00:00Z", "boolfield": true}});
    ///
    /// let expr: Expr = "boolfield and 1 + 2 = 3".parse().unwrap();
    /// assert_eq!(true, expr.matches(Some(&item)).unwrap());
    ///
    /// let expr: Expr = "eo:cloud_cover <= 9".parse().unwrap();
    /// assert_eq!(false, expr.matches(Some(&item)).unwrap());
    ///
    /// // A predicate that evaluates to NULL is not a match, and is not an error either.
    /// let expr: Expr = "null and true".parse().unwrap();
    /// assert_eq!(false, expr.matches(Some(&item)).unwrap());
    /// ```
    pub fn matches(self, j: Option<&Value>) -> Result<bool, Error> {
        let reduced = self.reduce(j)?;

        match reduced {
            Expr::Bool(v) => Ok(v),
            // A predicate is satisfied only when it is TRUE, so an unknown answer is not a match.
            // It is not an error: NULL is a value the expression legitimately evaluated to.
            Expr::Null => Ok(false),
            _ => Err(Error::NonReduced()),
        }
    }

    /// Returns True if the expression evaluates to true.
    ///
    /// Anything else is false: a predicate admits a record only when it is TRUE, so a NULL — the
    /// third truth value, which a comparison against a null value evaluates to — is not a match,
    /// and neither is an expression that did not fully reduce to a truth value.
    pub fn is_true(self) -> bool {
        matches!(self, Expr::Bool(true))
    }

    /// Filters an iterable of JSON values based on this expression.
    ///
    /// A record is kept only when the predicate is TRUE of it. A record the predicate is FALSE of,
    /// NULL of, or cannot be decided for — because a property it names is absent — is skipped
    /// rather than reported as an error.
    ///
    /// # Examples
    ///
    /// ```
    /// use cql2::Expr;
    /// use serde_json::json;
    ///
    /// let expr: Expr = "eo:cloud_cover < 20".parse().unwrap();
    /// let items = vec![
    ///     json!({"properties": {"eo:cloud_cover": 10}}),
    ///     json!({"properties": {"eo:cloud_cover": 25}}),
    ///     json!({"properties": {"eo:cloud_cover": 15}})
    /// ];
    /// let filtered = expr.filter(&items).unwrap();
    /// assert_eq!(filtered.len(), 2);
    /// assert_eq!(filtered[0]["properties"]["eo:cloud_cover"], 10);
    /// assert_eq!(filtered[1]["properties"]["eo:cloud_cover"], 15);
    /// ```
    pub fn filter<'a, I>(&self, items: I) -> Result<Vec<&'a Value>, Error>
    where
        I: IntoIterator<Item = &'a Value>,
    {
        let mut filtered = Vec::new();
        for item in items {
            let e = self.clone().reduce(Some(item))?;
            if e.is_true() {
                filtered.push(item)
            }
        }
        Ok(filtered)
    }

    /// Converts this expression to CQL2 text.
    ///
    /// # Examples
    ///
    /// ```
    /// use cql2::Expr;
    ///
    /// let expr = Expr::Bool(true);
    /// assert_eq!(expr.to_text().unwrap(), "true");
    /// ```
    pub fn to_text(&self) -> Result<String, Error> {
        macro_rules! check_len {
            ($name:expr, $args:expr, $len:expr, $text:expr) => {
                if $args.len() == $len {
                    Ok($text)
                } else {
                    Err(Error::InvalidNumberOfArguments {
                        name: $name.to_string(),
                        actual: $args.len(),
                        expected: $len,
                    })
                }
            };
        }

        match self {
            Expr::Bool(v) => Ok(v.to_string()),
            // An infinity or a NaN has no cql2-text literal, and the bare words `inf` and `NaN` the
            // default rendering writes would parse back as property names rather than as numbers.
            Expr::Float(v) if !v.is_finite() => Err(Error::NonFiniteNumber(*v)),
            Expr::Float(v) => Ok(v.to_string()),
            Expr::Literal(v) => Ok(literal(v)),
            Expr::Property { property } => Ok(identifier(property)),
            Expr::Null => Ok("NULL".to_string()),
            Expr::Interval { interval } => {
                check_len!(
                    "interval",
                    interval,
                    2,
                    format!(
                        "INTERVAL({},{})",
                        interval[0].to_text()?,
                        interval[1].to_text()?
                    )
                )
            }
            Expr::Date { date } => Ok(format!("DATE({})", date.to_text()?)),
            Expr::Timestamp { timestamp } => Ok(format!("TIMESTAMP({})", timestamp.to_text()?)),
            Expr::Geometry(v) => v.to_wkt(),
            Expr::Array(v) => {
                let array_els: Vec<String> =
                    v.iter().map(|a| a.to_text()).collect::<Result<_, _>>()?;
                Ok(format!("({})", array_els.join(", ")))
            }
            Expr::Operation { op, args } => {
                // Dispatch on the canonical spelling so a tree built without `normalize` renders
                // the same as a parsed one.
                let op = canonical_op(op);
                // Parenthesize only the operands that would otherwise re-associate when the text is
                // parsed back, so the rendering round-trips to an identical expression.
                let requirement = precedence::operands(&op);
                let a: Vec<String> = args
                    .iter()
                    .enumerate()
                    .map(|(index, arg)| {
                        let text = arg.to_text()?;
                        Ok(if requirement.needs_parens(index, arg) {
                            format!("({})", text)
                        } else {
                            text
                        })
                    })
                    .collect::<Result<_, Error>>()?;
                match op.as_str() {
                    "and" => Ok(a.join(" AND ")),
                    "or" => Ok(a.join(" OR ")),
                    "like" => {
                        check_len!("like", a, 2, format!("{} LIKE {}", a[0], a[1]))
                    }
                    "in" => {
                        check_len!("in", a, 2, format!("{} IN {}", a[0], a[1]))
                    }
                    "between" => {
                        check_len!(
                            "between",
                            a,
                            3,
                            format!("{} BETWEEN {} AND {}", a[0], a[1], a[2])
                        )
                    }
                    "not" => {
                        check_len!("not", a, 1, format!("NOT {}", a[0]))
                    }
                    "isNull" => {
                        check_len!("is null", a, 1, format!("{} IS NULL", a[0]))
                    }
                    "+" | "-" | "*" | "/" | "%" => {
                        // These chain their operands pairwise, so two is the floor: one operand
                        // would print as that operand by itself, with the operator silently
                        // dropped, and `{"op":"-","args":[{"property":"a"}]}` would render as `a`.
                        // `to_sql` requires the same, as `Arity::AtLeast(2)`.
                        if a.len() < 2 {
                            return Err(Error::InvalidNumberOfArguments {
                                name: op.to_string(),
                                actual: a.len(),
                                expected: 2,
                            });
                        }
                        let paddedop = format!(" {} ", op);
                        Ok(a.join(&paddedop))
                    }
                    "^" | "=" | "<=" | "<" | "<>" | ">" | ">=" => {
                        check_len!(op, a, 2, format!("{} {} {}", a[0], op, a[1]))
                    }
                    _ => Ok(format!("{}({})", identifier(&op), a.join(", "))),
                }
            }
            Expr::BBox { bbox } => {
                let array_els: Vec<String> =
                    bbox.iter().map(|a| a.to_text()).collect::<Result<_, _>>()?;
                Ok(format!("BBOX({})", array_els.join(", ")))
            }
        }
    }

    /// Converts this expression to a JSON string.
    ///
    /// # Examples
    ///
    /// ```
    /// use cql2::Expr;
    ///
    /// let expr = Expr::Bool(true);
    /// let s = expr.to_json().unwrap();
    /// ```
    pub fn to_json(&self) -> Result<String, Error> {
        serde_json::to_string(&self).map_err(Error::from)
    }

    /// Converts this expression to a pretty JSON string.
    ///
    /// # Examples
    ///
    /// ```
    /// use cql2::Expr;
    ///
    /// let expr = Expr::Bool(true);
    /// let s = expr.to_json_pretty().unwrap();
    /// ```
    pub fn to_json_pretty(&self) -> Result<String, Error> {
        serde_json::to_string_pretty(&self).map_err(Error::from)
    }

    /// Converts this expression to a [serde_json::Value].
    ///
    /// # Examples
    ///
    /// ```
    /// use cql2::Expr;
    ///
    /// let expr = Expr::Bool(true);
    /// let value = expr.to_value().unwrap();
    /// ```
    pub fn to_value(&self) -> Result<Value, Error> {
        serde_json::to_value(self).map_err(Error::from)
    }

    /// Returns true if this expression is valid CQL2.
    ///
    /// For detailed error reporting, use [Validator::validate] in conjunction with [Expr::to_value].
    ///
    /// # Examples
    ///
    /// ```
    /// use cql2::Expr;
    ///
    /// let expr = Expr::Bool(true);
    /// assert!(expr.is_valid());
    /// ```
    ///
    /// # Panics
    ///
    /// Panics if the default validator can't be created.
    pub fn is_valid(&self) -> bool {
        // Compiling the embedded schema is the expensive part, and the result is immutable, so
        // every call shares one validator.
        static VALIDATOR: OnceLock<Validator> = OnceLock::new();

        let value = serde_json::to_value(self);
        match &value {
            Ok(value) => {
                let validator = VALIDATOR
                    .get_or_init(|| Validator::new().expect("Could not create default validator"));
                validator.is_valid(value)
            }
            _ => false,
        }
    }
}

impl FromStr for Expr {
    type Err = Error;

    fn from_str(s: &str) -> Result<Expr, Error> {
        if s.starts_with('{') {
            crate::parse_json(s).map_err(Error::from)
        } else {
            crate::parse_text(s)
        }
    }
}

impl Add for Expr {
    type Output = Expr;

    ///
    /// Combines two expressions with the `+` operator.
    ///
    /// # Examples
    ///
    /// ```
    /// use cql2::Expr;
    /// use std::ops::Add;
    ///
    /// let expr1 = Expr::Bool(true);
    /// let expr2 = Expr::Bool(false);
    /// let expected_expr: Expr = "true and false".parse().unwrap();
    /// assert_eq!(expr1 + expr2, expected_expr);
    /// ```
    ///
    /// ```
    /// use cql2::Expr;
    /// use std::ops::Add;
    ///
    /// let expr1 = Expr::Bool(true);
    /// let expr2 = Expr::Bool(false);
    /// let expected_expr: Expr = "true and false".parse().unwrap();
    /// assert_eq!(expr1.add(expr2), expected_expr);
    /// ```
    fn add(self, other: Expr) -> Expr {
        Expr::Operation {
            op: "and".to_string(),
            args: vec![Box::new(self), Box::new(other)],
        }
    }
}
#[cfg(test)]
mod tests {
    use super::{canonical_op, canonical_ops, ALIASES, SPATIALOPS};
    use crate::Expr;
    use serde_json::Value;
    use std::collections::HashSet;

    /// Every spatial predicate is reachable by its SQL/PostGIS spelling, not just the ones that
    /// happened to be written down.
    #[test]
    fn every_spatial_operator_has_an_st_alias() {
        for op in SPATIALOPS {
            let st = format!("st_{}", op.trim_start_matches("s_"));
            assert_eq!(canonical_op(&st), *op, "'{st}' does not resolve to '{op}'");
        }
    }

    /// An alias has to name an operator that exists, or it silently invents one.
    #[test]
    fn aliases_resolve_to_canonical_operators() {
        let known: HashSet<&str> = canonical_ops().collect();
        for (alias, canonical) in ALIASES {
            assert!(
                known.contains(canonical),
                "'{alias}' resolves to '{canonical}', which is not an operator"
            );
        }
    }

    /// Aliases fold in both encodings, so the evaluator and both renderers see one operator.
    #[test]
    fn aliases_fold_in_both_encodings() {
        for (source, expected) in [
            ("ST_Intersects(geom, POINT(0 0))", "s_intersects"),
            ("st_intersects(geom, POINT(0 0))", "s_intersects"),
            ("INTERSECTS(geom, POINT(0 0))", "s_intersects"),
            ("AnyInteracts(a, b)", "t_intersects"),
            ("ST_CONTAINS(geom, POINT(0 0))", "s_contains"),
        ] {
            for text in [
                source.to_string(),
                // The same expression in cql2-json, built from the text spelling.
                {
                    let name = source.split('(').next().expect("has a name");
                    format!(r#"{{"op":"{name}","args":[{{"property":"a"}},{{"property":"b"}}]}}"#)
                },
            ] {
                let Ok(Expr::Operation { op, .. }) = text.parse::<Expr>() else {
                    panic!("{text} should parse to an operation");
                };
                assert_eq!(op, expected, "{text} did not fold to {expected}");
            }
        }
    }

    /// An n-ary arithmetic operator needs two operands to render, as it does in SQL.
    ///
    /// The rendering joins its operands with the operator, so a single operand would print as that
    /// operand alone and the operator would vanish: `{"op":"-","args":[{"property":"a"}]}` would
    /// render as `a`, which is a different expression that parses.
    #[test]
    fn nary_arithmetic_needs_two_operands() {
        for op in ["+", "-", "*", "/", "%"] {
            for count in [0, 1] {
                let expr = Expr::Operation {
                    op: op.to_string(),
                    args: (0..count)
                        .map(|i| {
                            Box::new(Expr::Property {
                                property: format!("a{i}"),
                            })
                        })
                        .collect(),
                };
                assert!(
                    matches!(
                        expr.to_text(),
                        Err(crate::Error::InvalidNumberOfArguments { .. })
                    ),
                    "{op} rendered {count} operand(s) as text: {:?}",
                    expr.to_text()
                );
                // The SQL backend has always rejected these; the two now agree.
                assert!(matches!(
                    crate::ToSqlAst::to_sql(&expr),
                    Err(crate::Error::InvalidNumberOfArguments { .. })
                ));
            }
            // Two operands is the floor, not a requirement of exactly two.
            let expr = Expr::Operation {
                op: op.to_string(),
                args: vec![
                    Box::new(Expr::Float(1.0)),
                    Box::new(Expr::Float(2.0)),
                    Box::new(Expr::Float(3.0)),
                ],
            };
            assert_eq!(
                expr.to_text().expect("three operands render"),
                format!("1 {op} 2 {op} 3")
            );
        }
    }

    /// An infinity or a NaN has no cql2-text spelling, so rendering one is an error.
    ///
    /// Rendered as the bare word `f64::to_string` writes, `inf` would parse back as a *property*
    /// named `inf` — a different expression that reads as valid CQL2, which is worse than a failure.
    #[test]
    fn non_finite_numbers_have_no_text() {
        for value in [f64::INFINITY, f64::NEG_INFINITY, f64::NAN] {
            assert!(
                matches!(
                    Expr::Float(value).to_text(),
                    Err(crate::Error::NonFiniteNumber(_))
                ),
                "{value} rendered as cql2-text"
            );
        }
        // Reachable from an expression that parsed: division by zero reduces to an infinity.
        let divided: Expr = "1 / 0".parse().unwrap();
        assert!(matches!(
            divided.reduce(None).unwrap().to_text(),
            Err(crate::Error::NonFiniteNumber(_))
        ));
        // An operand buried in a larger expression is reported the same way.
        let expr = Expr::Operation {
            op: ">".to_string(),
            args: vec![
                Box::new(Expr::Property {
                    property: "a".to_string(),
                }),
                Box::new(Expr::Float(f64::INFINITY)),
            ],
        };
        assert!(matches!(
            expr.to_text(),
            Err(crate::Error::NonFiniteNumber(_))
        ));
    }

    #[test]
    fn keep_z() {
        let point: Expr = "POINT Z(-105.1019 40.1672 4981)".parse().unwrap();
        assert_eq!("POINT Z(-105.1019 40.1672 4981)", point.to_text().unwrap());
    }

    #[test]
    fn implicit_z() {
        let point: Expr = "POINT (-105.1019 40.1672 4981)".parse().unwrap();
        assert_eq!("POINT Z(-105.1019 40.1672 4981)", point.to_text().unwrap());
    }

    #[test]
    fn keep_m() {
        let point: Expr = "POINT M(-105.1019 40.1672 42)".parse().unwrap();
        assert_eq!("POINT M(-105.1019 40.1672 42)", point.to_text().unwrap());
    }

    #[test]
    fn keep_zm() {
        let point: Expr = "POINT ZM(-105.1019 40.1672 4981 42)".parse().unwrap();
        assert_eq!(
            "POINT ZM(-105.1019 40.1672 4981 42)",
            point.to_text().unwrap()
        );
    }

    #[test]
    fn keep_one_element_lists() {
        // A single-element list stays a list rather than collapsing to a bare value.
        let expr: Expr = "ogc_fid IN ('1')".parse().unwrap();
        assert_eq!(expr.to_text().unwrap(), "ogc_fid IN ('1')");
    }

    /// The operator constants decide the spelling every name is resolved to, and the JSON schema
    /// enumerates those names case-sensitively. An operator missing here is emitted with whatever
    /// case it was written in, which the schema then reads as a function call rather than an
    /// operator, so the two lists have to agree.
    #[test]
    fn canonical_ops_match_the_schema() {
        let schema: Value =
            serde_json::from_str(include_str!("cql2.json")).expect("schema is valid JSON");

        let mut from_schema = HashSet::new();
        collect_operator_enums(&schema, &mut from_schema);
        assert!(
            !from_schema.is_empty(),
            "found no operator enums in the schema"
        );

        let known: HashSet<String> = canonical_ops().map(str::to_string).collect();
        let missing: Vec<&String> = from_schema.difference(&known).collect();
        assert!(
            missing.is_empty(),
            "these schema operators are absent from the operator constants: {missing:?}"
        );
    }

    /// Collects every `enum` in the schema that names operators, identified by containing an
    /// operator this crate is certain of.
    fn collect_operator_enums(node: &Value, out: &mut HashSet<String>) {
        match node {
            Value::Object(fields) => {
                if let Some(Value::Array(values)) = fields.get("enum") {
                    let names: Vec<String> = values
                        .iter()
                        .filter_map(|v| v.as_str().map(str::to_string))
                        .collect();
                    if names.iter().any(|n| n == "t_metBy" || n == "a_containedBy") {
                        out.extend(names);
                    }
                }
                for value in fields.values() {
                    collect_operator_enums(value, out);
                }
            }
            Value::Array(items) => items
                .iter()
                .for_each(|item| collect_operator_enums(item, out)),
            _ => {}
        }
    }
}
