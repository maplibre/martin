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
pub use crate::tiles::postgres::features::{
    PostgresFeature, PostgresProperty, PostgresTileFeatures, column_def, is_typed_property,
};

mod mlt_encoder;
pub use crate::tiles::postgres::mlt_encoder::{encode_features_as_mlt, keeps_measures};

mod source;
pub use crate::tiles::postgres::source::{PostgresRowQuery, PostgresSource, PostgresSqlInfo, PostgresTiler};

mod engine;
pub use crate::tiles::postgres::engine::PostgresEngineInfo;

mod scan;
pub use deadpool_postgres::Object as PooledConnection;

pub use crate::tiles::postgres::scan::{
    ScanLayout, estimated_rows, export_snapshot, heap_blocks, open_scan_connection, open_scan_connections,
    open_snapshot_connection, scan_features,
};

mod tile_wkb;
pub use crate::tiles::postgres::tile_wkb::{TileWkbError, parse_tile_wkb};

pub(crate) mod utils;
