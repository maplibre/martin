use std::fs::File;
use std::path::{Path, PathBuf};

use martin_tile_utils::Encoding;
use pmtiles::{CallerKey, Compression, PmTilesStreamWriter, PmTilesWriter, TileType};
use tilejson::TileJSON;

use super::{EncodedTile, TileSink};
use crate::{TileFormat, TileGenError, TileGenResult, TileOrder};

/// Writes a new `PMTiles` v3 archive in Hilbert order, clustered, with the engine's dedup keys
/// deciding which tiles share content, so the writer neither hashes every tile nor remembers it.
pub struct PmtilesSink {
    writer: PmTilesStreamWriter<File, CallerKey<u64>>,
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
            TileFormat::Mlt => TileType::Mlt,
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
            .precompressed(compression)
            .dedup_by_key::<u64>()
            .create(file)?;
        Ok(Self { writer })
    }
}

impl TileSink for PmtilesSink {
    fn tile_order(&self) -> TileOrder {
        TileOrder::Hilbert
    }

    fn write_all(&mut self, batches: &mut dyn Iterator<Item = TileGenResult<Vec<EncodedTile>>>) -> TileGenResult<()> {
        for batch in batches {
            for tile in batch? {
                let coord = pmtiles::TileCoord::new(tile.coord.z(), tile.coord.x(), tile.coord.y())?;
                self.writer.add_tile(coord, &tile.data, tile.dedup_key)?;
            }
        }
        Ok(())
    }

    fn finish(mut self, metadata: &TileJSON) -> TileGenResult<()> {
        self.writer
            .set_metadata(serde_json::to_string(metadata).map_err(std::io::Error::other)?);
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

    use super::*;

    #[test]
    fn refuses_zlib_and_existing_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.pmtiles");
        let mlt = TileFormat::Mlt;
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
        let tile = |x, y, data: &[u8], dedup_key| EncodedTile {
            coord: TileCoord::new_unchecked(1, x, y),
            data: data.to_vec(),
            dedup_key,
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
        let header = pmtiles::Header::try_from_bytes(std::fs::read(&path).unwrap().into()).unwrap();
        assert_eq!(header.n_addressed_tiles().map(u64::from), Some(3));
        assert_eq!(header.n_tile_entries().map(u64::from), Some(2));
        assert_eq!(header.n_tile_contents().map(u64::from), Some(2));
        assert!(header.clustered());
    }
}
