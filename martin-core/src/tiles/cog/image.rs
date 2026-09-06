use std::io::BufWriter;
use std::ops::Range;
use std::path::Path;
use std::sync::Arc;

use async_tiff::decoder::DecoderRegistry;
use async_tiff::tags::Compression;
use async_tiff::{CompressedBytes, ImageFileDirectory, TypedArray};
use martin_tile_utils::{Format, TileCoord, TileData};

use crate::tiles::cog::reader::AsyncTiffReader;
use crate::tiles::cog::{CogError, CogReader};

/// Image represents a single image in a COG file. A tiff file may contain many images.
/// This struct contains information and methods for taking tiles from the image.
#[derive(Clone, Debug)]
pub struct Image {
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
    compression: Compression,
    samples_per_pixel: u16,
    ifd: Arc<ImageFileDirectory>,
}

impl Image {
    #[expect(clippy::too_many_arguments)]
    pub fn new(
        zoom_level: u8,
        tiles_origin: (u32, u32),
        tiles_across: u32,
        tiles_down: u32,
        tile_size: u32,
        compression: Compression,
        samples_per_pixel: u16,
        ifd: Arc<ImageFileDirectory>,
    ) -> Self {
        Self {
            zoom_level,
            tiles_origin,
            tiles_across,
            tiles_down,
            tile_size,
            compression,
            samples_per_pixel,
            ifd,
        }
    }

    #[expect(
        clippy::wildcard_enum_match_arm,
        reason = "Compression is non-exhaustive and unsupported values all map to None"
    )]
    pub const fn output_format(&self) -> Option<Format> {
        match self.compression {
            Compression::WebP => Some(Format::Webp),
            Compression::ModernJPEG => Some(Format::Jpeg),
            Compression::Deflate
            | Compression::OldDeflate
            | Compression::LZW
            | Compression::None => Some(Format::Png),
            _ => None,
        }
    }

    const fn is_passthrough_compression(&self) -> bool {
        matches!(
            self.compression,
            Compression::WebP | Compression::ModernJPEG
        )
    }

    pub async fn get_tile(
        &self,
        reader: &Arc<dyn CogReader>,
        xyz: TileCoord,
        location: &str,
    ) -> Result<TileData, CogError> {
        let Some((tile_x, tile_y)) = self.get_tile_position(xyz) else {
            return Ok(Vec::new());
        };
        let tile_index = self
            .ifd
            .tile_count()
            .and_then(|(columns, _)| tile_y.checked_mul(columns))
            .and_then(|row_offset| row_offset.checked_add(tile_x))
            .ok_or_else(|| invalid_tile_table(location, "tile index overflow"))?;
        let tile_range = checked_tile_range(
            self.ifd.tile_offsets(),
            self.ifd.tile_byte_counts(),
            tile_index,
            location,
        )?;
        if tile_range.is_empty() {
            return Ok(Vec::new());
        }

        let tile = self
            .ifd
            .fetch_tile(tile_x, tile_y, &AsyncTiffReader(Arc::clone(reader)))
            .await
            .map_err(|e| CogError::AsyncTiff(e, location.to_owned()))?;

        if self.is_passthrough_compression() {
            let CompressedBytes::Chunky(bytes) = tile.compressed_bytes() else {
                return Err(CogError::UnsupportedPlanarLayout(location.to_owned()));
            };
            if self.compression == Compression::ModernJPEG
                && let Some(tables) = tile.jpeg_tables()
            {
                return Ok(merge_jpeg_tables_with_tile(tables, bytes));
            }
            return Ok(bytes.to_vec());
        }

        let array = tile
            .decode(&DecoderRegistry::default())
            .map_err(|e| CogError::AsyncTiff(e, location.to_owned()))?;
        let TypedArray::UInt8(pixels) = array.data() else {
            return Err(CogError::InvalidGeoInformation(
                Path::new(location).to_path_buf(),
                "Only 8-bit RGB/RGBA COG tiles are supported".to_owned(),
            ));
        };
        encode_as_png(self.tile_size, pixels, location, self.samples_per_pixel)
    }

    pub const fn compression(&self) -> Compression {
        self.compression
    }

    pub const fn tile_size(&self) -> u32 {
        self.tile_size
    }

    pub const fn zoom_level(&self) -> u8 {
        self.zoom_level
    }

    fn get_tile_position(&self, xyz: TileCoord) -> Option<(usize, usize)> {
        if xyz.z != self.zoom_level {
            return None;
        }
        let x = i64::from(xyz.x) - i64::from(self.tiles_origin.0);
        let y = i64::from(xyz.y) - i64::from(self.tiles_origin.1);
        if x < 0 || x >= i64::from(self.tiles_across) || y < 0 || y >= i64::from(self.tiles_down) {
            return None;
        }
        Some((usize::try_from(x).ok()?, usize::try_from(y).ok()?))
    }
}

fn checked_tile_range(
    offsets: Option<&[u64]>,
    byte_counts: Option<&[u64]>,
    index: usize,
    location: &str,
) -> Result<Range<u64>, CogError> {
    let offset = offsets
        .and_then(|values| values.get(index))
        .copied()
        .ok_or_else(|| invalid_tile_table(location, &format!("missing offset for tile {index}")))?;
    let byte_count = byte_counts
        .and_then(|values| values.get(index))
        .copied()
        .ok_or_else(|| {
            invalid_tile_table(location, &format!("missing byte count for tile {index}"))
        })?;
    let end = offset.checked_add(byte_count).ok_or_else(|| {
        invalid_tile_table(location, &format!("byte range overflow for tile {index}"))
    })?;
    Ok(offset..end)
}

fn invalid_tile_table(location: &str, reason: &str) -> CogError {
    CogError::InvalidGeoInformation(
        Path::new(location).to_path_buf(),
        format!("Invalid TIFF tile table: {reason}"),
    )
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
    if jpeg_tables.len() < 4 || tile_data.len() < 4 {
        return tile_data.to_vec();
    }
    if jpeg_tables[0..2] != JPEG_SOI || tile_data[0..2] != JPEG_SOI {
        return tile_data.to_vec();
    }
    let tables_end = if jpeg_tables[jpeg_tables.len() - 2..] == JPEG_EOI {
        jpeg_tables.len() - 2
    } else {
        jpeg_tables.len()
    };
    let tables_content = &jpeg_tables[2..tables_end];
    let mut result = Vec::with_capacity(2 + tables_content.len() + tile_data.len() - 2);
    result.extend_from_slice(&JPEG_SOI);
    result.extend_from_slice(tables_content);
    result.extend_from_slice(&tile_data[2..]);
    result
}

fn encode_as_png(
    tile_size: u32,
    pixels: &[u8],
    location: &str,
    samples_per_pixel: u16,
) -> Result<Vec<u8>, CogError> {
    let mut result = Vec::new();
    let color_type = match samples_per_pixel {
        3 => png::ColorType::Rgb,
        4 => png::ColorType::Rgba,
        _ => {
            return Err(CogError::InvalidGeoInformation(
                Path::new(location).to_path_buf(),
                format!("Unsupported samples per pixel: {samples_per_pixel}"),
            ));
        }
    };
    {
        let mut encoder = png::Encoder::new(BufWriter::new(&mut result), tile_size, tile_size);
        encoder.set_color(color_type);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .map_err(|e| CogError::WritePngHeaderFailed(Path::new(location).to_path_buf(), e))?;
        writer
            .write_image_data(pixels)
            .map_err(|e| CogError::WriteToPngFailed(Path::new(location).to_path_buf(), e))?;
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::{checked_tile_range, merge_jpeg_tables_with_tile};

    #[test]
    fn malformed_tile_tables_return_errors() {
        checked_tile_range(Some(&[]), Some(&[10]), 0, "image.tif").unwrap_err();
        checked_tile_range(Some(&[1]), Some(&[]), 0, "image.tif").unwrap_err();
        checked_tile_range(Some(&[u64::MAX]), Some(&[1]), 0, "image.tif").unwrap_err();
        assert_eq!(
            checked_tile_range(Some(&[7]), Some(&[0]), 0, "image.tif").unwrap(),
            7..7
        );
    }

    #[test]
    fn can_merge_jpeg_tables_with_tile() {
        let jpeg_tables = vec![
            0xFF, 0xD8, 0xFF, 0xDB, 0x00, 0x05, 0x00, 0x10, 0x20, 0xFF, 0xD9,
        ];
        let tile_data = vec![
            0xFF, 0xD8, 0xFF, 0xC0, 0x00, 0x04, 0x08, 0x10, 0xFF, 0xDA, 0x00, 0x02, 0x12, 0x34,
            0x56, 0xFF, 0xD9,
        ];
        let expected = vec![
            0xFF, 0xD8, 0xFF, 0xDB, 0x00, 0x05, 0x00, 0x10, 0x20, 0xFF, 0xC0, 0x00, 0x04, 0x08,
            0x10, 0xFF, 0xDA, 0x00, 0x02, 0x12, 0x34, 0x56, 0xFF, 0xD9,
        ];
        assert_eq!(
            merge_jpeg_tables_with_tile(&jpeg_tables, &tile_data),
            expected
        );
    }

    #[test]
    fn merge_returns_tile_data_for_invalid_tables() {
        let tile_data = vec![0xFF, 0xD8, 0xFF, 0xC0, 0x00, 0x02, 0xFF, 0xD9];
        assert_eq!(
            merge_jpeg_tables_with_tile(&[0xFF, 0xD8], &tile_data),
            tile_data
        );
        assert_eq!(
            merge_jpeg_tables_with_tile(&[0, 0, 0, 0], &tile_data),
            tile_data
        );
    }
}
