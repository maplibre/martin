//! Transport-independent classification of failures.
//!
//! Errors here describe *what went wrong* in the vocabulary of the subsystem that failed.
//! A transport edge needs a different question answered: *whose fault was it, and is a
//! retry worth trying?* [`ErrorKind`] is that second axis, so an edge maps one small enum
//! instead of re-matching every subsystem's variants.

/// How a caller should treat a failure, independent of which subsystem produced it.
///
/// Deliberately not `#[non_exhaustive]`: adding a kind should break every edge that maps it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ErrorKind {
    /// The requested resource does not exist.
    NotFound,
    /// The request was malformed, out of range, or asked for something unsupported.
    InvalidInput,
    /// A dependency was reachable but failed to serve the request; a retry may succeed.
    Unavailable,
    /// A defect in martin, or a failure that callers cannot act on.
    Internal,
}

/// Answers [`ErrorKind`] for an error type.
///
/// Prefer widening to [`ErrorKind::Internal`] over guessing: a misclassified
/// [`ErrorKind::NotFound`] hides a real defect behind a 404.
pub trait Classify {
    /// Classifies this failure for a caller that must map it onto a transport.
    fn kind(&self) -> ErrorKind;
}
