use std::fmt::{Debug, Formatter};
use std::io;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use log::{trace, warn};
use martin_tile_utils::{Encoding, Format, TileInfo};
use pmtiles::async_reader::AsyncPmTilesReader;
use pmtiles::mmap::MmapBackend;
use pmtiles::{Compression, TileType};
use tilejson::TileJSON;

use crate::file_config::FileError::{InvalidMetadata, IoError};
use crate::file_config::FileResult;
use crate::source::{Source, UrlQuery};
use crate::{MartinResult, TileCoord, TileData};

#[derive(Clone)]
pub struct PmtFileSource {
    id: String,
    path: PathBuf,
    pmtiles: Arc<AsyncPmTilesReader<MmapBackend>>,
    tilejson: TileJSON,
    tile_info: TileInfo,
}

impl Debug for PmtFileSource {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "PmtFileSource {{ id: {}, path: {:?} }}",
            self.id, self.path
        )
    }
}

impl PmtFileSource {
    pub async fn new_box(id: String, path: PathBuf) -> FileResult<Box<dyn Source>> {
        Ok(Box::new(PmtFileSource::new(id, path).await?))
    }

    async fn new(id: String, path: PathBuf) -> FileResult<Self> {
        let backend = MmapBackend::try_from(path.as_path())
            .await
            .map_err(|e| {
                io::Error::new(
                    io::ErrorKind::Other,
                    format!("{e:?}: Cannot open file {}", path.display()),
                )
            })
            .map_err(|e| IoError(e, path.clone()))?;

        let reader = AsyncPmTilesReader::try_from_source(backend).await;
        let reader = reader
            .map_err(|e| {
                io::Error::new(
                    io::ErrorKind::Other,
                    format!("{e:?}: Cannot open file {}", path.display()),
                )
            })
            .map_err(|e| IoError(e, path.clone()))?;

        Self::new_int(id, path, reader).await
    }
}

impl PmtFileSource {
    async fn new_int(
        id: String,
        path: PathBuf,
        reader: AsyncPmTilesReader<MmapBackend>,
    ) -> FileResult<Self> {
        let hdr = &reader.get_header();

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
            TileType::Mvt => TileInfo::new(
                Format::Mvt,
                match hdr.tile_compression {
                    Compression::None => Encoding::Uncompressed,
                    Compression::Unknown => {
                        warn!(
                            "MVT tiles have unknown compression in file {}",
                            path.display()
                        );
                        Encoding::Uncompressed
                    }
                    Compression::Gzip => Encoding::Gzip,
                    Compression::Brotli => Encoding::Brotli,
                    Compression::Zstd => Encoding::Zstd,
                },
            ),
            // All these assume uncompressed data (validated above)
            TileType::Png => Format::Png.into(),
            TileType::Jpeg => Format::Jpeg.into(),
            TileType::Webp => Format::Webp.into(),
            TileType::Unknown => {
                return Err(InvalidMetadata(
                    "Unknown tile type".to_string(),
                    path.clone(),
                ))
            }
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
            tile_info: format,
        })
    }
}

#[async_trait]
impl Source for PmtFileSource {
    fn get_id(&self) -> &str {
        &self.id
    }

    fn get_tilejson(&self) -> &TileJSON {
        &self.tilejson
    }

    fn get_tile_info(&self) -> TileInfo {
        self.tile_info
    }

    fn clone_source(&self) -> Box<dyn Source> {
        Box::new(self.clone())
    }

    async fn get_tile(
        &self,
        xyz: &TileCoord,
        _url_query: &Option<UrlQuery>,
    ) -> MartinResult<TileData> {
        // TODO: optimize to return Bytes
        if let Some(t) = self
            .pmtiles
            .get_tile(xyz.z, u64::from(xyz.x), u64::from(xyz.y))
            .await
        {
            Ok(t.to_vec())
        } else {
            trace!(
                "Couldn't find tile data in {}/{}/{} of {}",
                xyz.z,
                xyz.x,
                xyz.y,
                &self.id
            );
            Ok(Vec::new())
        }
    }
}
