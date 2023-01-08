use std::fmt::{Debug, Formatter};
use std::io;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use log::warn;
use martin_tile_utils::DataFormat;
use pmtiles::async_reader::AsyncPmTilesReader;
use pmtiles::mmap::MmapBackend;
use pmtiles::{Compression, TileType};
use tilejson::TileJSON;

use crate::file_config::FileError;
use crate::file_config::FileError::{GetTileError, InvalidMetadata};
use crate::source::{Source, Tile, UrlQuery, Xyz};
use crate::utils::is_valid_zoom;
use crate::Error;

#[derive(Clone)]
pub struct PmtSource {
    id: String,
    path: PathBuf,
    pmtiles: Arc<AsyncPmTilesReader<MmapBackend>>,
    tilejson: TileJSON,
    format: DataFormat,
}

impl Debug for PmtSource {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "PmtSource {{ id: {}, path: {:?} }}", self.id, self.path)
    }
}

impl PmtSource {
    pub async fn new_box(id: String, path: PathBuf) -> Result<Box<dyn Source>, FileError> {
        Ok(Box::new(PmtSource::new(id, path).await?))
    }

    async fn new(id: String, path: PathBuf) -> Result<Self, FileError> {
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
        let hdr = &reader.header;

        if hdr.tile_type != TileType::Mvt && hdr.tile_compression != Compression::None {
            return Err(InvalidMetadata(
                format!(
                    "Format {:?} and compression {:?} are not yet supported",
                    hdr.tile_type, hdr.tile_compression
                ),
                path,
            ));
        }

        let format = match hdr.tile_type {
            TileType::Unknown => DataFormat::Unknown,
            TileType::Mvt => match hdr.tile_compression {
                Compression::Unknown | Compression::None => DataFormat::Mvt,
                Compression::Gzip => DataFormat::GzipMvt,
                Compression::Brotli => DataFormat::BrotliMvt,
                Compression::Zstd => DataFormat::ZstdMvt,
            },
            TileType::Png => DataFormat::Png,
            TileType::Jpeg => DataFormat::Jpeg,
            TileType::Webp => DataFormat::Webp,
        };

        let tilejson = reader.parse_tilejson(Vec::new()).await.unwrap_or_else(|e| {
            warn!("{e:?}: Unable to parse metadata for {}", path.display());
            hdr.get_tilejson(Vec::new())
        });

        Ok(Self {
            id,
            path,
            pmtiles: Arc::new(reader),
            tilejson,
            format,
        })
    }
}

#[async_trait]
impl Source for PmtSource {
    fn get_tilejson(&self) -> TileJSON {
        self.tilejson.clone()
    }

    fn get_format(&self) -> DataFormat {
        self.format
    }

    fn clone_source(&self) -> Box<dyn Source> {
        Box::new(self.clone())
    }

    fn is_valid_zoom(&self, zoom: u8) -> bool {
        is_valid_zoom(zoom, self.tilejson.minzoom, self.tilejson.maxzoom)
    }

    fn support_url_query(&self) -> bool {
        false
    }

    async fn get_tile(&self, xyz: &Xyz, _url_query: &Option<UrlQuery>) -> Result<Tile, Error> {
        // TODO: optimize to return Bytes
        Ok(self
            .pmtiles
            .get_tile(xyz.z, u64::from(xyz.x), u64::from(xyz.y))
            .await
            .ok_or_else(|| GetTileError(*xyz, self.id.clone()))?
            .data
            .to_vec())
    }
}
