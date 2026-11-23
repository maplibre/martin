//! Turning JPEG-in-TIFF tiles into standalone JPEG images.

use bytes::{Bytes, BytesMut};

const SOI: [u8; 2] = [0xFF, 0xD8];
const EOI: [u8; 2] = [0xFF, 0xD9];

/// Inserts the shared `JPEGTables` (quantization and Huffman tables) into an abbreviated tile.
///
/// In TIFF, `JPEGTables` is `SOI + tables + EOI` and each tile is `SOI + frame + scan + EOI`
/// without the tables. A standalone image is `SOI + tables + frame + scan + EOI`.
/// Data that does not look like JPEG is returned unchanged.
pub(crate) fn merge_tables(tables: &[u8], tile: Bytes) -> Bytes {
    if tables.len() < 4 || tile.len() < 4 || tables[..2] != SOI || tile[..2] != SOI {
        return tile;
    }
    let tables_end = if tables[tables.len() - 2..] == EOI {
        tables.len() - 2
    } else {
        tables.len()
    };
    let tables = &tables[2..tables_end];
    let mut merged = BytesMut::with_capacity(tables.len() + tile.len());
    merged.extend_from_slice(&SOI);
    merged.extend_from_slice(tables);
    merged.extend_from_slice(&tile[2..]);
    merged.freeze()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merges_tables_between_soi_and_frame() {
        let tables = [0xFF, 0xD8, 0xFF, 0xDB, 0x00, 0x05, 0x00, 0x10, 0x20, 0xFF, 0xD9];
        let tile = Bytes::from_static(&[
            0xFF, 0xD8, 0xFF, 0xC0, 0x00, 0x04, 0x08, 0x10, 0xFF, 0xDA, 0x00, 0x02, 0x12, 0xFF, 0xD9,
        ]);
        let expected = [
            0xFF, 0xD8, 0xFF, 0xDB, 0x00, 0x05, 0x00, 0x10, 0x20, 0xFF, 0xC0, 0x00, 0x04, 0x08, 0x10, 0xFF, 0xDA, 0x00,
            0x02, 0x12, 0xFF, 0xD9,
        ];
        assert_eq!(merge_tables(&tables, tile).as_ref(), expected);
    }

    #[test]
    fn leaves_non_jpeg_data_alone() {
        let tile = Bytes::from_static(&[0xFF, 0xD8, 0xFF, 0xC0, 0xFF, 0xD9]);
        assert_eq!(merge_tables(&[0xFF, 0xD8], tile.clone()), tile);
        assert_eq!(merge_tables(&[0, 0, 0, 0], tile.clone()), tile);
    }
}
