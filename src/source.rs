use crate::pg::db::Connection;
use std::collections::HashMap;
use std::fmt::Debug;
use std::io;
use tilejson::TileJSON;

pub type Tile = Vec<u8>;
pub type UrlQuery = HashMap<String, String>;

#[derive(Copy, Clone)]
pub struct Xyz {
    pub z: i32,
    pub x: i32,
    pub y: i32,
}

pub trait Source: Debug {
    fn get_id(&self) -> impl std::future::Future<Output = &str> + Send;

    fn get_tilejson(&self)
        -> impl std::future::Future<Output = Result<TileJSON, io::Error>> + Send;

    fn get_tile(
        &self,
        conn: &mut Connection,
        xyz: &Xyz,
        query: &Option<UrlQuery>,
    ) -> impl std::future::Future<Output = Result<Tile, io::Error>> + Send;
}
