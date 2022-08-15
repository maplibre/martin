use std::collections::HashMap;
use std::fmt::Debug;
use std::io;

use async_trait::async_trait;
use tilejson::TileJSON;

use crate::db::Connection;

pub type Tile = Vec<u8>;
pub type Query = HashMap<String, String>;

#[derive(Copy, Clone)]
pub struct Xyz {
    pub z: i32,
    pub x: i32,
    pub y: i32,
}

#[async_trait]
pub trait Source: Debug {
    async fn get_id(&self) -> &str;

    async fn get_tilejson(&self) -> Result<TileJSON, io::Error>;

    async fn get_tile(
        &self,
        conn: &mut Connection,
        xyz: &Xyz,
        query: &Option<Query>,
    ) -> Result<Tile, io::Error>;
}
