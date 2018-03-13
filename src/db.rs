use actix::prelude::{Actor, Handler, Message, SyncContext};
use r2d2_postgres::{PostgresConnectionManager, TlsMode};
use r2d2::{Config, Pool, PooledConnection};
use std::error::Error;
use std::io;

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
}

impl Message for GetTile {
    type Result = Result<String, io::Error>;
}

impl Handler<GetTile> for DbExecutor {
    type Result = Result<String, io::Error>;

    fn handle(&mut self, msg: GetTile, _: &mut Self::Context) -> Self::Result {
        debug!("self {:?}\n\n\n", self);
        debug!("msg {:?}\n\n\n", msg);

        // let conn = self.0.get().unwrap();

        // let tile: Vec<u8> = match conn.query(&query, &[]) {
        //     Ok(rows) => rows.get(0).get("st_asmvt"),
        //     Err(error) => {
        //         debug!("{} 500", url);
        //         error!("Couldn't get a tile: {}", error);
        //         return Ok(Response::with(status::InternalServerError));
        //     }
        // };

        // Ok(
        //     conn.query_row("SELECT name FROM users WHERE id=$1", &[&uuid], |row| {
        //         row.get(0)
        //     }).map_err(|_| io::Error::new(io::ErrorKind::Other, "db error"))?,
        // )

        Ok("result".to_string())
    }
}
