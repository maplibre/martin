//! Which rows of a table its layers keep, as a condition a source can check before reading them.

use crate::plan::GeometryType;

/// The type every non-null value of a column has, as an expression sees it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValueKind {
    Bool,
    Int,
    Float,
    String,
}

/// The rows a source has to read for a table.
#[derive(Clone, Debug, PartialEq)]
pub enum RowFilter {
    /// Every row: some layer takes all of them.
    All,
    /// Exactly the rows matching this.
    Matching(Condition),
    /// Every row, as some layer filters on something a [`Condition`] cannot express.
    Untranslatable(Untranslatable),
}

impl RowFilter {
    /// The condition a source filters on, if any.
    #[must_use]
    pub fn condition(&self) -> Option<&Condition> {
        match self {
            Self::Matching(condition) => Some(condition),
            Self::All | Self::Untranslatable(_) => None,
        }
    }
}

/// A layer's `where` that no [`Condition`] expresses.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
#[error("layer `{layer}`: `{expr}` {reason}")]
pub struct Untranslatable {
    pub layer: String,
    pub expr: String,
    pub reason: TranslateError,
}

/// Why an expression has no [`Condition`].
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum TranslateError {
    #[error("reads columns that a key of `{0}` may replace (list `attributes` without its keys)")]
    KeysMayReplaceColumns(String),
    #[error("reads `{0}`, which is not a column the source can filter on")]
    NotAColumn(String),
    #[error("reads `feature` by a computed key")]
    ComputedKey,
    #[error("selects a field of a value")]
    SelectsField,
    #[error("uses {0}, which has no SQL equivalent")]
    Expression(&'static str),
    #[error("uses `{0}`, which has no SQL equivalent")]
    Function(String),
    #[error("calls `{0}` on something other than a string column")]
    NotAStringColumn(String),
    #[error("calls `{0}` without a literal")]
    NotALiteral(String),
    #[error("compares `{0}`, whose type the source cannot compare")]
    UnknownColumnType(String),
    #[error("compares two constants")]
    Constants,
    #[error("compares two float columns, which may hold NaN")]
    FloatColumns,
    #[error("compares a function of a column to a column")]
    FunctionToColumn,
    #[error("orders something other than a column against a literal")]
    Order,
    #[error("looks up a constant in a list")]
    ConstantInList,
    #[error("uses `in` without a list of literals")]
    NotALiteralList,
    #[error("uses a string holding a NUL character")]
    NulInString,
    #[error("uses a bytes literal")]
    Bytes,
    #[error("negates the smallest integer")]
    NegatedMinimum,
    #[error("negates something other than a number")]
    NegatedNonNumber,
}

/// A condition on a row with SQL's three-valued logic: the row passes only when it is true.
#[derive(Clone, Debug, PartialEq)]
pub enum Condition {
    Const(bool),
    And(Vec<Self>),
    Or(Vec<Self>),
    /// Both sides compared as `kind`, strings by code point.
    Compare {
        left: Term,
        op: CompareOp,
        right: Term,
        kind: ValueKind,
    },
    /// `IS DISTINCT FROM`, or `IS NOT DISTINCT FROM` once negated, both sides compared as `kind`.
    Distinct {
        left: Term,
        right: Term,
        kind: ValueKind,
        polarity: Polarity,
    },
    /// `IS NULL`, or `IS NOT NULL` once negated.
    IsNull {
        term: Term,
        polarity: Polarity,
    },
    /// `IN`, or `NOT IN` once negated, over a non-empty list of values compared as `kind`.
    In {
        term: Term,
        values: Vec<Literal>,
        kind: ValueKind,
        polarity: Polarity,
    },
    /// A string matching the pattern, or not matching it once negated.
    Like {
        term: Term,
        pattern: Pattern,
        polarity: Polarity,
    },
    /// The geometry is of this type, single or multi.
    Geometry(GeometryType),
}

/// Whether a condition holds as written or negated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Polarity {
    AsWritten,
    Negated,
}

impl Polarity {
    /// The other polarity.
    pub(crate) fn flip(self) -> Self {
        match self {
            Self::AsWritten => Self::Negated,
            Self::Negated => Self::AsWritten,
        }
    }

    /// Whether this is [`Polarity::Negated`].
    pub(crate) fn negated(self) -> bool {
        self == Self::Negated
    }

    /// `op`, or its negation.
    pub(crate) fn op(self, op: CompareOp) -> CompareOp {
        match self {
            Self::AsWritten => op,
            Self::Negated => op.negate(),
        }
    }

    /// All of `parts`, or any of them once negated.
    pub(crate) fn all(self, parts: impl IntoIterator<Item = Condition>) -> Condition {
        match self {
            Self::AsWritten => Condition::and(parts),
            Self::Negated => Condition::or(parts),
        }
    }

    /// `NOT ` once negated, as SQL writes it before `DISTINCT`, `NULL`, `IN` or `LIKE`.
    #[must_use]
    pub fn sql(self) -> &'static str {
        match self {
            Self::AsWritten => "",
            Self::Negated => "NOT ",
        }
    }

    /// Any of `parts`, or all of them once negated.
    pub(crate) fn any(self, parts: impl IntoIterator<Item = Condition>) -> Condition {
        self.flip().all(parts)
    }
}

/// A comparison operator.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompareOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

impl CompareOp {
    /// The operator true exactly where this one is false, for values that compare.
    pub(crate) fn negate(self) -> Self {
        match self {
            Self::Eq => Self::Ne,
            Self::Ne => Self::Eq,
            Self::Lt => Self::Ge,
            Self::Le => Self::Gt,
            Self::Gt => Self::Le,
            Self::Ge => Self::Lt,
        }
    }

    /// The operator with its sides swapped.
    pub(crate) fn mirror(self) -> Self {
        match self {
            Self::Eq | Self::Ne => self,
            Self::Lt => Self::Gt,
            Self::Le => Self::Ge,
            Self::Gt => Self::Lt,
            Self::Ge => Self::Le,
        }
    }

    /// The operator as SQL.
    #[must_use]
    pub fn sql(self) -> &'static str {
        match self {
            Self::Eq => "=",
            Self::Ne => "<>",
            Self::Lt => "<",
            Self::Le => "<=",
            Self::Gt => ">",
            Self::Ge => ">=",
        }
    }
}

/// A value of a row.
#[derive(Clone, Debug, PartialEq)]
pub enum Term {
    /// A column, by its [`ColumnDef`](super::ColumnDef) name.
    Column(String),
    Literal(Literal),
    /// The number of characters of a string.
    Length(Box<Self>),
    /// A string with its ASCII letters lowercased.
    LowerAscii(Box<Self>),
    /// A string with its ASCII letters uppercased.
    UpperAscii(Box<Self>),
}

/// A constant value.
#[derive(Clone, Debug, PartialEq)]
pub enum Literal {
    Bool(bool),
    Int(i64),
    UInt(u64),
    Float(f64),
    String(String),
}

/// A string pattern, matched character by character.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pattern {
    Prefix(String),
    Suffix(String),
    Contains(String),
}

impl Condition {
    /// `parts` all hold, flattened, with constants folded.
    pub(crate) fn and(parts: impl IntoIterator<Item = Self>) -> Self {
        Self::fold(parts, true)
    }

    /// One of `parts` holds, flattened, with constants folded.
    pub(crate) fn or(parts: impl IntoIterator<Item = Self>) -> Self {
        Self::fold(parts, false)
    }

    /// `and` when `all`, else `or`.
    fn fold(parts: impl IntoIterator<Item = Self>, all: bool) -> Self {
        let mut kept = Vec::new();
        for part in parts {
            match (part, all) {
                (Self::Const(value), _) if value == all => {}
                (Self::Const(_), _) => return Self::Const(!all),
                (Self::And(inner), true) | (Self::Or(inner), false) => kept.extend(inner),
                (part, _) => kept.push(part),
            }
        }
        match kept.len() {
            0 => Self::Const(all),
            1 => kept.pop().expect("one part"),
            _ if all => Self::And(kept),
            _ => Self::Or(kept),
        }
    }
}
