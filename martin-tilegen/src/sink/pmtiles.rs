use std::fs::File;
use std::path::{Path, PathBuf};

use martin_tile_utils::Encoding;
use pmtiles::{Compression, PmTilesStreamWriter, PmTilesWriter, TileType};
use tilejson::TileJSON;

use super::{EncodedTile, TileSink};
use crate::{TileFormat, TileGenError, TileGenResult, TileOrder};

/// Writes a new `PMTiles` v3 archive in Hilbert order, clustered, with the engine's dedup keys
/// deciding which tiles share content, so the writer neither hashes every tile nor remembers it.
pub struct PmtilesSink {
    writer: PmTilesStreamWriter<File>,
}

impl PmtilesSink {
    pub fn create(path: &Path, format: TileFormat, encoding: Encoding) -> TileGenResult<Self> {
        let compression = match encoding {
            Encoding::Uncompressed | Encoding::Internal => Compression::None,
            Encoding::Gzip => Compression::Gzip,
            Encoding::Brotli => Compression::Brotli,
            Encoding::Zstd => Compression::Zstd,
            Encoding::Zlib => return Err(TileGenError::UnsupportedEncoding(encoding)),
        };
        let tile_type = match format {
            TileFormat::Mlt(_) => TileType::Mlt,
            TileFormat::Mvt => TileType::Mvt,
        };
        let file = File::create_new(path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::AlreadyExists {
                TileGenError::OutputNotEmpty(PathBuf::from(path))
            } else {
                e.into()
            }
        })?;
        let writer = PmTilesWriter::new(tile_type)
            .tile_compression(compression)
            .create(file)?;
        Ok(Self { writer })
    }
}

impl TileSink for PmtilesSink {
    fn tile_order(&self) -> TileOrder {
        TileOrder::Hilbert
    }

    fn write_all(
        &mut self,
        batches: &mut dyn Iterator<Item = TileGenResult<Vec<EncodedTile>>>,
    ) -> TileGenResult<()> {
        for batch in batches {
            for tile in batch? {
                let coord =
                    pmtiles::TileCoord::new(tile.coord.z(), tile.coord.x(), tile.coord.y())?;
                self.writer
                    .add_raw_tile_with_dedup(coord, &tile.data, tile.dedup)?;
            }
        }
        Ok(())
    }

    fn finish(mut self, metadata: &TileJSON) -> TileGenResult<()> {
        self.writer
            .set_metadata(&serde_json::to_string(metadata).map_err(std::io::Error::other)?);
        let (min, max) = (metadata.minzoom.unwrap_or(0), metadata.maxzoom.unwrap_or(0));
        self.writer.set_zoom_range(min, max);
        let bounds = metadata.bounds.unwrap_or_default();
        self.writer
            .set_bounds(bounds.left, bounds.bottom, bounds.right, bounds.top);
        let (lon, lat) = (
            f64::midpoint(bounds.left, bounds.right),
            f64::midpoint(bounds.bottom, bounds.top),
        );
        self.writer.set_center(lon, lat, min);
        Ok(self.writer.finalize()?)
    }
}

#[cfg(test)]
mod tests {
    use martin_tile_utils::TileCoord;
    use mlt_core::encoder::EncoderConfig;

    use super::*;

    #[test]
    fn refuses_zlib_and_existing_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.pmtiles");
        let mlt = TileFormat::Mlt(EncoderConfig::default());
        assert!(matches!(
            PmtilesSink::create(&path, mlt, Encoding::Zlib),
            Err(TileGenError::UnsupportedEncoding(_))
        ));
        drop(PmtilesSink::create(&path, mlt, Encoding::Gzip).unwrap());
        assert!(matches!(
            PmtilesSink::create(&path, mlt, Encoding::Gzip),
            Err(TileGenError::OutputNotEmpty(_))
        ));
    }

    #[test]
    fn writes_tiles_with_shared_content() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.pmtiles");
        let mut sink = PmtilesSink::create(&path, TileFormat::Mvt, Encoding::Uncompressed).unwrap();
        let tile = |x, y, data: &[u8], dedup| EncodedTile {
            coord: TileCoord::new_unchecked(1, x, y),
            data: data.to_vec(),
            dedup,
        };
        // In Hilbert order, as the engine writes them.
        let batch = vec![
            tile(0, 0, b"fill", Some(1)),
            tile(0, 1, b"fill", Some(1)),
            tile(1, 1, b"edge", None),
        ];
        sink.write_all(&mut std::iter::once(Ok(batch))).unwrap();
        let mut metadata = tilejson::tilejson! { tiles: vec![] };
        (metadata.minzoom, metadata.maxzoom) = (Some(1), Some(1));
        sink.finish(&metadata).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(&bytes[..7], b"PMTiles");
        // Header field at offset 72: number of addressed tiles.
        assert_eq!(u64::from_le_bytes(bytes[72..80].try_into().unwrap()), 3);
    }
}
