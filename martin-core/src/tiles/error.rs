/// A convenience [`Result`] for tiles coming from `martin-core`.
pub type MartinCoreResult<T> = Result<T, MartinCoreError>;

/// Temporary error type for integration purposes.
pub type MartinCoreError = Box<dyn std::error::Error>;
