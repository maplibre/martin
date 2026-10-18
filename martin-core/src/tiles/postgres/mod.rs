mod connection_string;
pub use connection_string::RedactedConnectionString;

mod errors;
pub use errors::{PostgresError, PostgresResult};

mod tls;

mod pool;
pub use pool::{ActiveQueryRegistry, PostgresPool};

mod retry_timeout;
pub use retry_timeout::RetryTimeout;

mod features;
pub use features::{PostgresFeature, PostgresProperty, PostgresTileFeatures, is_typed_property};

mod mlt_encoder;
pub use mlt_encoder::{encode_features_as_mlt, keeps_measures};

mod source;
pub use source::{PostgresRowQuery, PostgresSource, PostgresSqlInfo, PostgresTiler};

#[cfg(feature = "unstable-generate")]
mod engine;
#[cfg(feature = "unstable-generate")]
pub use engine::PostgresEngineInfo;

#[cfg(feature = "unstable-generate")]
mod scan;
#[cfg(feature = "unstable-generate")]
pub use deadpool_postgres::Object as PooledConnection;
#[cfg(feature = "unstable-generate")]
pub use scan::{
    ScanLayout, heap_blocks, open_scan_connection, open_scan_connections, scan_features,
};

mod tile_wkb;
pub use tile_wkb::{TileWkbError, parse_tile_wkb};

pub(crate) mod utils;
