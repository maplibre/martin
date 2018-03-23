use actix::prelude::{Actor, Handler, Message, SyncContext};
use r2d2_postgres::{PostgresConnectionManager, TlsMode};
use r2d2::{Config, Pool, PooledConnection};
use std::error::Error;
use std::io;

use super::utils;
use super::source::Source;

// static GET_SOURCES_QUERY: &'static str = include_str!("scripts/get_sources.sql");

pub type PostgresPool = Pool<PostgresConnectionManager>;
pub type PostgresConnection = PooledConnection<PostgresConnectionManager>;

pub fn setup_connection_pool(cn_str: &str, pool_size: u32) -> Result<PostgresPool, Box<Error>> {
    let config = Config::builder().pool_size(pool_size).build();
    let manager = PostgresConnectionManager::new(cn_str, TlsMode::None)?;
    let pool = Pool::new(config, manager)?;
    Ok(pool)
}

#[derive(Debug)]
pub struct DbExecutor(pub PostgresPool);

impl Actor for DbExecutor {
    type Context = SyncContext<Self>;
}

#[derive(Debug)]
pub struct GetTile {
    pub z: u32,
    pub x: u32,
    pub y: u32,
    pub source: Source,
    pub condition: Option<String>,
}

impl Message for GetTile {
    type Result = Result<Vec<u8>, io::Error>;
}

impl Handler<GetTile> for DbExecutor {
    type Result = Result<Vec<u8>, io::Error>;

    fn handle(&mut self, msg: GetTile, _: &mut Self::Context) -> Self::Result {
        let conn = self.0.get().unwrap();
        let source = msg.source;

        let mercator_bounds = utils::tilebbox(msg.z, msg.x, msg.y);

        let (geometry_column_mercator, original_bounds) = if source.srid == 3857 {
            (source.geometry_column.clone(), mercator_bounds.clone())
        } else {
            (
                format!("ST_Transform({0}, 3857)", source.geometry_column),
                format!("ST_Transform({0}, {1})", mercator_bounds, source.srid),
            )
        };

        let columns: Vec<String> = source
            .properties
            .keys()
            .map(|column| format!("\"{0}\"", column))
            .collect();

        let properties = columns.join(",");

        let condition = msg.condition
            .map_or("".to_string(), |condition| format!("AND {}", condition));

        let query = format!(
            include_str!("scripts/get_tile.sql"),
            id = source.id,
            geometry_column = source.geometry_column,
            geometry_column_mercator = geometry_column_mercator,
            original_bounds = original_bounds,
            mercator_bounds = mercator_bounds,
            extent = source.extent,
            buffer = source.buffer,
            clip_geom = source.clip_geom,
            properties = properties,
            condition = condition,
        );

        let tile: Vec<u8> = conn.query(&query, &[])
            .map(|rows| rows.get(0).get("st_asmvt"))
            .map_err(|_| io::Error::new(io::ErrorKind::Other, "db error"))?;

        Ok(tile)
    }
}
