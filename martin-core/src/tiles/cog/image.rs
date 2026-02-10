use std::fs::File;
use std::io::{BufWriter, Read as _, Seek as _, SeekFrom};
use std::path::Path;

use martin_tile_utils::{Format, TileCoord, TileData};
use tiff::tags::CompressionMethod;
use tiff::{ColorType, decoder::Decoder};

use crate::tiles::cog::CogError;

/// WEBP compression code (not in standard TIFF, registered by GDAL)
pub const COMPRESSION_WEBP: u16 = 50001;

/// Image represents a single image in a COG file. A tiff file may contain many images.
/// This struct contains information and methods for taking tiles from the image.
#[derive(Clone, Debug)]
pub struct Image {
    /// The Image File Directory index represents IFD entry with the image pointers to the actual image data.
    ifd_index: usize,
    /// Zoom level which this image corresponds to
    zoom_level: u8,
    /// X and Y of the first tile in this image
    tiles_origin: (u32, u32),
    /// Number of tiles in a row of this image
    tiles_across: u32,
    /// Number of tiles in a column of this image
    tiles_down: u32,
    /// Tile size in pixels
    tile_size: u32,
    /// Compression method used for tiles
    compression: u16,
}

impl Image {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        ifd_index: usize,
        zoom_level: u8,
        tiles_origin: (u32, u32),
        tiles_across: u32,
        tiles_down: u32,
        tile_size: u32,
        compression: u16,
    ) -> Self {
        Self {
            ifd_index,
            zoom_level,
            tiles_origin,
            tiles_across,
            tiles_down,
            tile_size,
            compression,
        }
    }

    /// Returns the output format for this image based on compression.
    pub fn output_format(&self) -> Option<Format> {
        if self.compression == COMPRESSION_WEBP {
            return Some(Format::Webp);
        }
        match CompressionMethod::from_u16(self.compression) {
            Some(CompressionMethod::ModernJPEG) => Some(Format::Jpeg),
            Some(CompressionMethod::Deflate | CompressionMethod::LZW | CompressionMethod::None) => {
                Some(Format::Png)
            }
            _ => None,
        }
    }

    /// Returns true if this image uses a passthrough compression (WEBP or JPEG)
    /// where raw tile bytes can be returned directly without re-encoding.
    fn is_passthrough_compression(&self) -> bool {
        if self.compression == COMPRESSION_WEBP {
            return true;
        }
        matches!(
            CompressionMethod::from_u16(self.compression),
            Some(CompressionMethod::ModernJPEG)
        )
    }

    pub fn get_tile(
        &self,
        decoder: &mut Decoder<File>,
        xyz: TileCoord,
        path: &Path,
    ) -> Result<TileData, CogError> {
        decoder
            .seek_to_image(self.ifd_index)
            .map_err(|e| CogError::IfdSeekFailed(e, self.ifd_index, path.to_path_buf()))?;

        let Some(idx) = self.get_chunk_index(xyz) else {
            return Ok(Vec::new());
        };

        // For WEBP and JPEG compression, return the raw tile bytes directly
        if self.is_passthrough_compression() {
            return self.read_raw_tile_bytes(decoder, idx, path);
        }

        // For other compression types (LZW, Deflate, None), decode and re-encode as PNG
        let color_type = decoder
            .colortype()
            .map_err(|e| CogError::InvalidTiffFile(e, path.to_path_buf()))?;

        let mut pixels = vec![
            0;
            (self.tile_size * self.tile_size * u32::from(color_type.num_samples()))
                as usize
        ];
        if decoder.read_chunk_bytes(idx, &mut pixels).is_err() {
            return Ok(Vec::new());
        }

        let png = encode_as_png(self.tile_size(), &pixels, path, color_type)?;
        Ok(png)
    }

    /// Reads the raw bytes of a tile directly from the TIFF file without decompression.
    /// This is used for WEBP and JPEG compressed tiles where we can pass through the bytes.
    fn read_raw_tile_bytes(
        &self,
        decoder: &mut Decoder<File>,
        chunk_index: u32,
        path: &Path,
    ) -> Result<TileData, CogError> {
        use tiff::tags::Tag;

        // For JPEG compression, we may need to merge JPEGTables with tile data
        let jpeg_tables = if CompressionMethod::from_u16(self.compression)
            == Some(CompressionMethod::ModernJPEG)
        {
            decoder.get_tag_u8_vec(Tag::JPEGTables).ok()
        } else {
            None
        };

        // Get tile offsets and byte counts from the TIFF tags
        let tile_offsets = decoder
            .get_tag_u64_vec(Tag::TileOffsets)
            .map_err(|e| CogError::InvalidTiffFile(e, path.to_path_buf()))?;
        let tile_byte_counts = decoder
            .get_tag_u64_vec(Tag::TileByteCounts)
            .map_err(|e| CogError::InvalidTiffFile(e, path.to_path_buf()))?;

        let idx = chunk_index as usize;
        if idx >= tile_offsets.len() || idx >= tile_byte_counts.len() {
            return Ok(Vec::new());
        }

        let offset = tile_offsets[idx];
        let byte_count = usize::try_from(tile_byte_counts[idx]).unwrap_or(0);

        // If byte count is 0, this is an empty/sparse tile
        if byte_count == 0 {
            return Ok(Vec::new());
        }

        // Seek to the tile offset and read the raw bytes
        let file = decoder.inner();
        file.seek(SeekFrom::Start(offset))
            .map_err(|e| CogError::IoError(e, path.to_path_buf()))?;

        let mut tile_data = vec![0u8; byte_count];
        file.read_exact(&mut tile_data)
            .map_err(|e| CogError::IoError(e, path.to_path_buf()))?;

        // For JPEG, merge JPEGTables with tile data if tables are present
        if let Some(tables) = jpeg_tables {
            return Ok(merge_jpeg_tables_with_tile(&tables, &tile_data));
        }

        Ok(tile_data)
    }

    pub fn compression(&self) -> u16 {
        self.compression
    }

    pub fn tile_size(&self) -> u32 {
        self.tile_size
    }

    pub fn zoom_level(&self) -> u8 {
        self.zoom_level
    }

    fn get_chunk_index(&self, xyz: TileCoord) -> Option<u32> {
        if xyz.z != self.zoom_level {
            return None;
        }

        let x = i64::from(xyz.x) - i64::from(self.tiles_origin.0);
        let y = i64::from(xyz.y) - i64::from(self.tiles_origin.1);
        if 0 > x || x >= i64::from(self.tiles_across) || 0 > y || y >= i64::from(self.tiles_down) {
            return None;
        }

        let idx = y * i64::from(self.tiles_across) + x;
        u32::try_from(idx).ok()
    }
}

/// JPEG marker constants
const JPEG_SOI: [u8; 2] = [0xFF, 0xD8]; // Start of Image
const JPEG_EOI: [u8; 2] = [0xFF, 0xD9]; // End of Image

/// Merges JPEG tables (from `JPEGTables` tag) with tile data to create a valid standalone JPEG.
///
/// In TIFF JPEG compression, the quantization and Huffman tables are often stored
/// separately in the `JPEGTables` tag and shared across all tiles. Each tile then only
/// contains the frame data without these tables.
///
/// `JPEGTables` format: SOI (FFD8) + tables (DQT, DHT, etc.) + EOI (FFD9)
/// Tile data format: SOI (FFD8) + frame header + scan data + EOI (FFD9)
///
/// To merge: Take tables (without SOI/EOI) and insert after tile's SOI, before frame data.
fn merge_jpeg_tables_with_tile(jpeg_tables: &[u8], tile_data: &[u8]) -> Vec<u8> {
    // Validate minimum sizes
    if jpeg_tables.len() < 4 || tile_data.len() < 4 {
        return tile_data.to_vec();
    }

    // Verify both start with SOI marker
    if jpeg_tables[0..2] != JPEG_SOI || tile_data[0..2] != JPEG_SOI {
        return tile_data.to_vec();
    }

    // Extract tables content (skip SOI at start, skip EOI at end if present)
    let tables_end = if jpeg_tables.len() >= 2 && jpeg_tables[jpeg_tables.len() - 2..] == JPEG_EOI {
        jpeg_tables.len() - 2
    } else {
        jpeg_tables.len()
    };
    let tables_content = &jpeg_tables[2..tables_end];

    // Build merged JPEG: SOI + tables + tile data (without SOI)
    let mut result = Vec::with_capacity(2 + tables_content.len() + tile_data.len() - 2);
    result.extend_from_slice(&JPEG_SOI);
    result.extend_from_slice(tables_content);
    result.extend_from_slice(&tile_data[2..]); // Skip tile's SOI

    result
}

/// Encodes RGBA pixel data to PNG format.
fn encode_as_png(
    tile_size: u32,
    pixels: &[u8],
    path: &Path,
    color_type: ColorType,
) -> Result<Vec<u8>, CogError> {
    let mut result_file_buffer = Vec::new();
    let png_color_type = match color_type {
        ColorType::RGB(8) => Ok(png::ColorType::Rgb),
        ColorType::RGBA(8) => Ok(png::ColorType::Rgba),
        c => Err(CogError::NotSupportedColorTypeAndBitDepth(
            c,
            path.to_path_buf(),
        )),
    }?;

    {
        let mut encoder = png::Encoder::new(
            BufWriter::new(&mut result_file_buffer),
            tile_size,
            tile_size,
        );
        encoder.set_color(png_color_type);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .map_err(|e| CogError::WritePngHeaderFailed(path.to_path_buf(), e))?;
        writer
            .write_image_data(pixels)
            .map_err(|e| CogError::WriteToPngFailed(path.to_path_buf(), e))?;
    }

    Ok(result_file_buffer)
}

#[cfg(test)]
mod tests {
    use crate::tiles::cog::image::{Image, merge_jpeg_tables_with_tile};
    use martin_tile_utils::TileCoord;

    #[test]
    fn can_calculate_correct_chunk_index() {
        let image = Image {
            ifd_index: 0,
            zoom_level: 0,
            tiles_origin: (0, 0),
            tiles_across: 3,
            tiles_down: 3,
            tile_size: 256,
            compression: 1, // None
        };
        assert_eq!(
            Some(0),
            image.get_chunk_index(TileCoord { z: 0, x: 0, y: 0 })
        );
        assert_eq!(None, image.get_chunk_index(TileCoord { z: 2, x: 2, y: 2 }));
        assert_eq!(None, image.get_chunk_index(TileCoord { z: 0, x: 3, y: 0 }));
        assert_eq!(None, image.get_chunk_index(TileCoord { z: 0, x: 1, y: 9 }));
    }

    #[test]
    fn can_merge_jpeg_tables_with_tile() {
        // JPEGTables: SOI + table data + EOI
        let jpeg_tables = vec![
            0xFF, 0xD8, // SOI
            0xFF, 0xDB, 0x00, 0x05, 0x00, 0x10, 0x20, // DQT marker with some data
            0xFF, 0xD9, // EOI
        ];

        // Tile data: SOI + frame data + EOI
        let tile_data = vec![
            0xFF, 0xD8, // SOI
            0xFF, 0xC0, 0x00, 0x04, 0x08, 0x10, // SOF marker with some data
            0xFF, 0xDA, 0x00, 0x02, // SOS marker
            0x12, 0x34, 0x56, // compressed data
            0xFF, 0xD9, // EOI
        ];

        let merged = merge_jpeg_tables_with_tile(&jpeg_tables, &tile_data);

        // Expected: SOI + table data (no EOI) + frame data (no SOI) + EOI
        let expected = vec![
            0xFF, 0xD8, // SOI (from tables)
            0xFF, 0xDB, 0x00, 0x05, 0x00, 0x10, 0x20, // DQT marker (tables content)
            0xFF, 0xC0, 0x00, 0x04, 0x08, 0x10, // SOF marker (tile without SOI)
            0xFF, 0xDA, 0x00, 0x02, // SOS marker
            0x12, 0x34, 0x56, // compressed data
            0xFF, 0xD9, // EOI
        ];

        assert_eq!(merged, expected);
    }

    #[test]
    fn merge_returns_tile_data_when_tables_too_short() {
        let jpeg_tables = vec![0xFF, 0xD8]; // Just SOI, too short
        let tile_data = vec![0xFF, 0xD8, 0xFF, 0xC0, 0x00, 0x02, 0xFF, 0xD9];

        let merged = merge_jpeg_tables_with_tile(&jpeg_tables, &tile_data);
        assert_eq!(merged, tile_data);
    }

    #[test]
    fn merge_returns_tile_data_when_invalid_markers() {
        let jpeg_tables = vec![0x00, 0x00, 0x00, 0x00]; // No SOI
        let tile_data = vec![0xFF, 0xD8, 0xFF, 0xC0, 0x00, 0x02, 0xFF, 0xD9];

        let merged = merge_jpeg_tables_with_tile(&jpeg_tables, &tile_data);
        assert_eq!(merged, tile_data);
    }
}
