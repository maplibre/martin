use async_trait::async_trait;
use martin_tile_utils::DataFormat;
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

#[async_trait]
pub trait Source: Send + Debug {
    fn get_tilejson(&self) -> TileJSON;

    fn get_format(&self) -> DataFormat;

    fn clone_source(&self) -> Box<dyn Source>;

    fn is_valid_zoom(&self, zoom: i32) -> bool;

    async fn get_tile(&self, xyz: &Xyz, query: &Option<UrlQuery>) -> Result<Tile, io::Error>;
}

impl Clone for Box<dyn Source> {
    fn clone(&self) -> Self {
        self.clone_source()
    }
}
