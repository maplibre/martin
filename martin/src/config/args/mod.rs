mod error;
pub use crate::config::args::error::{ArgsError, ArgsResult};

mod connections;
pub use crate::config::args::connections::State;

#[cfg(any(feature = "postgres", feature = "unstable-duckdb"))]
mod bounds;
#[cfg(any(feature = "postgres", feature = "unstable-duckdb"))]
mod order_by;
#[cfg(any(feature = "postgres", feature = "unstable-duckdb"))]
pub use crate::config::args::bounds::{BoundsCalcType, DEFAULT_BOUNDS_TIMEOUT};
#[cfg(any(feature = "postgres", feature = "unstable-duckdb"))]
pub use crate::config::args::order_by::OrderByType;

#[cfg(feature = "postgres")]
mod postgres;
#[cfg(feature = "postgres")]
pub use crate::config::args::postgres::PostgresArgs;

mod root;
pub use root::*;

mod srv;
pub use srv::*;
