use std::error::Error;
use iron::typemap::Key;
use iron::prelude::{Plugin, Request};
use persistent::Read;
use r2d2::{Config, Pool, PooledConnection};
use r2d2_postgres::{TlsMode, PostgresConnectionManager};

pub type PostgresPool = Pool<PostgresConnectionManager>;
pub type PostgresPooledConnection = PooledConnection<PostgresConnectionManager>;

pub struct DB;
impl Key for DB { type Value = PostgresPool; }

pub fn setup_connection_pool(cn_str: &str, pool_size: u32) -> Result<PostgresPool, Box<Error>> {
    let config = Config::builder().pool_size(pool_size).build();
    let manager = try!(PostgresConnectionManager::new(cn_str, TlsMode::None));
    let pool = try!(Pool::new(config, manager));
    Ok(pool)
}

pub fn get_connection(req: &mut Request) -> Result<PostgresPooledConnection, Box<Error>> {
    let pool = try!(req.get::<Read<DB>>());
    let conn = try!(pool.get());
    Ok(conn)
}

pub fn get_tile(conn: PostgresPooledConnection, schema: &str, table: &str, z: &str, x: &str, y: &str) -> Result<Vec<u8>, Box<Error>> {
    let query = format!(
        "SELECT ST_AsMVT(q, '{1}', 4096, 'geom') FROM ( \
            SELECT ST_AsMVTGeom(                        \
                geom,                                   \
                TileBBox({2}, {3}, {4}, 4326),          \
                4096,                                   \
                256,                                    \
                true                                    \
            ) AS geom FROM {0}.{1}                      \
        ) AS q;",
        schema, table, z, x, y
    );

    let rows = try!(conn.query(&query, &[]));
    let tile = rows.get(0).get("st_asmvt");
    Ok(tile)
}