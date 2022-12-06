use crate::pg::utils::{create_tilejson, io_error, is_valid_zoom};
use crate::source::{Source, Tile, UrlQuery, Xyz};
use async_trait::async_trait;
use martin_tile_utils::DataFormat;
use pmtiles::async_reader::AsyncPmTilesReader;
use pmtiles::mmap::MmapBackend;
use std::fmt::{Debug, Formatter};
use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use tilejson::TileJSON;

#[derive(Clone)]
pub struct PmtSource {
    id: String,
    path: PathBuf,
    pmtiles: Arc<AsyncPmTilesReader<MmapBackend>>,
    tilejson: TileJSON,
}

impl Debug for PmtSource {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "PmtSource {{ id: {}, path: {:?} }}", self.id, self.path)
    }
}

impl PmtSource {
    pub async fn new(id: String, path: PathBuf) -> io::Result<Self> {
        let backend = MmapBackend::try_from(path.as_path()).await.map_err(|e| {
            io::Error::new(
                io::ErrorKind::Other,
                format!("{e:?}: Cannot open file {}", path.display()),
            )
        })?;

        let reader = AsyncPmTilesReader::try_from_source(backend).await;
        let reader = reader.map_err(|e| {
            io::Error::new(
                io::ErrorKind::Other,
                format!("{e:?}: Cannot open file {}", path.display()),
            )
        })?;
        Ok(Self {
            id: id.clone(),
            path,
            pmtiles: Arc::new(reader),
            tilejson: create_tilejson(id, None, None, None, None),
        })
    }
}

#[async_trait]
impl Source for PmtSource {
    fn get_tilejson(&self) -> TileJSON {
        self.tilejson.clone()
    }

    fn get_format(&self) -> DataFormat {
        DataFormat::Mvt
    }

    fn clone_source(&self) -> Box<dyn Source> {
        Box::new(self.clone())
    }

    fn is_valid_zoom(&self, zoom: i32) -> bool {
        is_valid_zoom(zoom, self.tilejson.minzoom, self.tilejson.maxzoom)
    }

    async fn get_tile(&self, xyz: &Xyz, _url_query: &Option<UrlQuery>) -> Result<Tile, io::Error> {
        Ok(self
            .pmtiles
            .get_tile(xyz.z as u8, xyz.x as u64, xyz.y as u64)
            .await
            .ok_or_else(|| io_error!("Tile {xyz} not found"))?
            .data)
    }
}
