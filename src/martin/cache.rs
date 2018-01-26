use iron::typemap::Key;
use iron::url::Url;
use lru::LruCache;

pub struct TileCache;
impl Key for TileCache { type Value = LruCache<Url, Vec<u8>>; }