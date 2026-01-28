use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

use martin_tile_utils::{TileCoord, TileData};
use tiff::decoder::Decoder;

use crate::tiles::cog::CogError;

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
}

impl Image {
    pub fn new(
        ifd_index: usize,
        zoom_level: u8,
        tiles_origin: (u32, u32),
        tiles_across: u32,
        tiles_down: u32,
        tile_size: u32,
    ) -> Self {
        Self {
            ifd_index,
            zoom_level,
            tiles_origin,
            tiles_across,
            tiles_down,
            tile_size,
        }
    }

    #[allow(clippy::cast_possible_truncation)]
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
            Err(CogError::NoImagesFound(path.to_path_buf()))?
        };
        let color_type = decoder
            .colortype()
            .map_err(|e| CogError::InvalidTiffFile(e, path.to_path_buf()))?;

        // @todo: can we replace this with reading the raw bytes and
        // sending them over the wire with the correct header instead?
        let mut target = vec![
            0;
            (self.tile_size * self.tile_size * u32::from(color_type.num_samples()))
                as usize
        ];
        if decoder.read_chunk_bytes(idx, &mut target).is_err() {
            return Ok(Vec::new());
        }

        let png = encode_rgba_as_png(self.tile_size(), &target, path)?;
        Ok(png)
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
        if x < 0 || x >= i64::from(self.tiles_across) || y < 0 || y >= i64::from(self.tiles_down) {
            return None;
        }

        let idx = y * i64::from(self.tiles_across) + x;
        u32::try_from(idx).ok()
    }
}

/// Encodes RGBA pixel data to PNG format.
fn encode_rgba_as_png(tile_size: u32, pixels: &[u8], path: &Path) -> Result<Vec<u8>, CogError> {
    let mut result_file_buffer = Vec::new();

    {
        let mut encoder = png::Encoder::new(
            BufWriter::new(&mut result_file_buffer),
            tile_size,
            tile_size,
        );
        encoder.set_color(png::ColorType::Rgba);
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
    use crate::tiles::cog::image::Image;
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
        };
        assert_eq!(
            Some(0),
            image.get_chunk_index(TileCoord { z: 0, x: 0, y: 0 })
        );
        assert_eq!(None, image.get_chunk_index(TileCoord { z: 2, x: 2, y: 2 }));
        assert_eq!(None, image.get_chunk_index(TileCoord { z: 0, x: 3, y: 0 }));
        assert_eq!(None, image.get_chunk_index(TileCoord { z: 0, x: 1, y: 9 }));
    }
}
