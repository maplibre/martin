mod connection_string;
pub use crate::tiles::postgres::connection_string::RedactedConnectionString;

mod errors;
pub use crate::tiles::postgres::errors::{PostgresError, PostgresResult};

mod tls;

mod pool;
pub use crate::tiles::postgres::pool::{ActiveQueryRegistry, PostgresPool};

mod retry_timeout;
pub use crate::tiles::postgres::retry_timeout::RetryTimeout;

mod features;
#[cfg(feature = "unstable-generate")]
pub use crate::tiles::postgres::features::column_def;
pub use crate::tiles::postgres::features::{
    PostgresFeature, PostgresProperty, PostgresTileFeatures, is_typed_property,
};

mod mlt_encoder;
pub use crate::tiles::postgres::mlt_encoder::{encode_features_as_mlt, keeps_measures};

mod source;
pub use crate::tiles::postgres::source::{
    PostgresRowQuery, PostgresSource, PostgresSqlInfo, PostgresTiler,
};

#[cfg(feature = "unstable-generate")]
mod engine;
#[cfg(feature = "unstable-generate")]
pub use crate::tiles::postgres::engine::PostgresEngineInfo;

#[cfg(feature = "unstable-generate")]
mod scan;
#[cfg(feature = "unstable-generate")]
pub use deadpool_postgres::Object as PooledConnection;

#[cfg(feature = "unstable-generate")]
pub use crate::tiles::postgres::scan::{
    ScanLayout, heap_blocks, open_scan_connection, open_scan_connections, scan_features,
};

mod tile_wkb;
pub use crate::tiles::postgres::tile_wkb::{TileWkbError, parse_tile_wkb};

pub(crate) mod utils;
