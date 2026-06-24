//! Convert between an on-disk tile directory tree (`{z}/{x}/{y}.{ext}`) and an `MBTiles` archive.
//!
//! [`pack`] walks a directory tree and writes the tiles into a flat `MBTiles` file, while
//! [`unpack`] streams the tiles out of an `MBTiles` file back into such a directory tree.

use std::path::Path;

use futures::StreamExt as _;
use martin_tile_utils::{Encoding, Format, TileInfo, decode_gzip, decode_zlib, encode_gzip};
use walkdir::WalkDir;

use crate::{
    CopyDuplicateMode, MbtError, MbtResult, MbtType, Mbtiles, UpdateZoomType, create_flat_tables,
    create_metadata_table, invert_y_value,
};

/// Number of tiles buffered in memory before they are flushed to the `MBTiles` file in one batch.
const PACK_BATCH_SIZE: usize = 1000;

/// Tile-coordinate scheme of an on-disk tile directory tree.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "cli", derive(clap::ValueEnum))]
pub enum TileScheme {
    /// XYZ (aka. "slippy map") scheme where Y=0 is at the top
    #[cfg_attr(feature = "cli", value(name = "xyz"))]
    #[default]
    Xyz,
    /// TMS scheme where Y=0 is at the bottom
    #[cfg_attr(feature = "cli", value(name = "tms"))]
    Tms,
}

/// How [`pack`] compresses tiles before storing them in the `MBTiles` file.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "cli", derive(clap::ValueEnum))]
pub enum PackCompression {
    /// Gzip vector tiles and store everything else as-is, matching `MBTiles` conventions
    #[cfg_attr(feature = "cli", value(name = "auto"))]
    #[default]
    Auto,
    /// Store every tile uncompressed
    #[cfg_attr(feature = "cli", value(name = "none"))]
    None,
    /// Gzip-compress every tile
    #[cfg_attr(feature = "cli", value(name = "gzip", alias = "gz"))]
    Gzip,
}

/// Packs a `{z}/{x}/{y}.{ext}` directory tree at `input_directory` into a flat `MBTiles` file at
/// `output_file`, interpreting the directory layout with `scheme` and compressing tiles per
/// `compression`.
pub async fn pack(
    input_directory: &Path,
    output_file: &Path,
    scheme: TileScheme,
    compression: PackCompression,
) -> MbtResult<()> {
    let mbt = Mbtiles::new(output_file)?;
    let mut conn = mbt.open_or_new().await?;

    create_metadata_table(&mut conn, false).await?;
    create_flat_tables(&mut conn, false).await?;

    // Warn at most once per category so a misnamed tree does not flood the log.
    let mut warned_about_dirs = false;
    let mut warned_about_files = false;
    let walker = WalkDir::new(input_directory).follow_links(true);
    let entries = walker.into_iter().filter_entry(|entry| {
        if entry.file_type().is_dir() {
            // descend into the root and numerically-named `{z}`/`{x}` directories only
            let keep = entry.depth() == 0
                || entry
                    .file_name()
                    .to_str()
                    .is_some_and(|s| s.parse::<u32>().is_ok());
            if !keep && !warned_about_dirs {
                tracing::info!(
                    "Skipping {} and similarly-named directories; expected numeric `z`/`x` directory names",
                    entry.path().display()
                );
                warned_about_dirs = true;
            }
            keep
        } else {
            // keep files whose stem is numeric (`{y}.{ext}`)
            let keep = entry
                .path()
                .file_stem()
                .and_then(|s| s.to_str())
                .is_some_and(|s| s.parse::<u32>().is_ok());
            if !keep && !warned_about_files {
                tracing::info!(
                    "Skipping {} and similarly-named files; expected numeric `y.<ext>` file names",
                    entry.path().display()
                );
                warned_about_files = true;
            }
            keep
        }
    });

    let mut format: Option<Format> = None;
    let mut batch: Vec<(u8, u32, u32, Vec<u8>)> = Vec::with_capacity(PACK_BATCH_SIZE);

    for entry in entries {
        let entry = entry.map_err(std::io::Error::from)?;
        if entry.file_type().is_dir() {
            continue;
        }
        let path = entry.path();
        let Some((z, x, y)) = tile_coords(path) else {
            continue;
        };

        let detected = path
            .extension()
            .and_then(|e| e.to_str())
            .and_then(Format::parse)
            .ok_or_else(|| MbtError::UnsupportedFileExtension(path.to_path_buf()))?;
        match format {
            None => format = Some(detected),
            Some(f) if f != detected => {
                return Err(MbtError::InconsistentTileFormats {
                    old: f,
                    new: detected,
                    path: path.to_path_buf(),
                });
            }
            Some(_) => {}
        }

        let data = std::fs::read(path)?;
        // `auto` follows the MBTiles convention of gzipping vector tiles and leaving
        // raster tiles untouched; explicit choices apply to every tile.
        let target = match compression {
            PackCompression::Auto if detected == Format::Mvt => Encoding::Gzip,
            PackCompression::Auto | PackCompression::None => Encoding::Uncompressed,
            PackCompression::Gzip => Encoding::Gzip,
        };
        let encoded = recode_tile(data, target)?;

        // `insert_tiles` expects XYZ `y` and stores it as TMS internally.
        let y = match scheme {
            TileScheme::Xyz => y,
            TileScheme::Tms => invert_y_value(z, y),
        };

        batch.push((z, x, y, encoded));
        if batch.len() >= PACK_BATCH_SIZE {
            mbt.insert_tiles(&mut conn, MbtType::Flat, CopyDuplicateMode::Abort, &batch)
                .await?;
            batch.clear();
        }
    }
    if !batch.is_empty() {
        mbt.insert_tiles(&mut conn, MbtType::Flat, CopyDuplicateMode::Abort, &batch)
            .await?;
    }

    if let Some(format) = format {
        mbt.set_metadata_value(&mut conn, "format", format.metadata_format_value())
            .await?;
    }

    // Derive minzoom/maxzoom (and the compression key) and the geographic bounds from the
    // tiles we just inserted.
    mbt.update_metadata(&mut conn, UpdateZoomType::Reset)
        .await?;
    if let Some(bbox) = mbt.summary(&mut conn).await?.bbox {
        mbt.set_metadata_value(&mut conn, "bounds", bbox).await?;
    }

    Ok(())
}

/// Unpacks the `MBTiles` file at `input_file` into a `{z}/{x}/{y}.{ext}` directory tree under
/// `output_directory`, laying out the `y` coordinate according to `scheme`.
pub async fn unpack(
    input_file: &Path,
    output_directory: &Path,
    scheme: TileScheme,
) -> MbtResult<()> {
    if !input_file.exists() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("Input file does not exist: {}", input_file.display()),
        )
        .into());
    }

    let mbt = Mbtiles::new(input_file)?;
    let mut conn = mbt.open_readonly().await?;

    // Derive the output file extension from the stored format.
    let format = mbt.get_metadata_value(&mut conn, "format").await?;
    let Some(format_str) = format.as_deref() else {
        return Err(MbtError::NoFormatInMetadata(input_file.to_path_buf()));
    };
    let extension = Format::parse(format_str)
        .ok_or_else(|| MbtError::UnknownFormatInMetadata {
            format: format_str.to_string(),
            path: input_file.to_path_buf(),
        })?
        .metadata_format_value();

    std::fs::create_dir_all(output_directory)?;

    let mut tiles = mbt.stream_tiles(&mut conn);
    while let Some(tile) = tiles.next().await {
        // `stream_tiles` already validates the indices and yields XYZ coordinates.
        let (coord, data) = tile?;
        let Some(data) = data else { continue };

        let y = match scheme {
            TileScheme::Xyz => coord.y,
            TileScheme::Tms => invert_y_value(coord.z, coord.y),
        };

        // Vector tiles are stored gzip-compressed; write them back out decompressed.
        let data = if TileInfo::detect(&data).encoding == Encoding::Gzip {
            decode_gzip(&data)?
        } else {
            data
        };

        let tile_dir = output_directory
            .join(coord.z.to_string())
            .join(coord.x.to_string());
        std::fs::create_dir_all(&tile_dir)?;
        std::fs::write(tile_dir.join(format!("{y}.{extension}")), &data)?;
    }

    // TODO: write metadata.json file with minzoom, maxzoom, bounds, etc?

    Ok(())
}

/// Parses the `{z}/{x}/{y}` coordinates out of a tile path, ignoring any extension and leading
/// path components. Returns [`None`] if the trailing three components are not numeric.
fn tile_coords(path: &Path) -> Option<(u8, u32, u32)> {
    let y = path.file_stem()?.to_str()?.parse::<u32>().ok()?;
    let mut dirs = path.ancestors().skip(1);
    let x = dirs.next()?.file_name()?.to_str()?.parse::<u32>().ok()?;
    let z = dirs.next()?.file_name()?.to_str()?.parse::<u8>().ok()?;
    Some((z, x, y))
}

/// Re-encodes `data` so it ends up in `target` encoding, decoding any existing
/// compression first so we never double-compress. `Internal` (PNG/JPEG/WebP) is
/// already plaintext for our purposes.
fn recode_tile(data: Vec<u8>, target: Encoding) -> MbtResult<Vec<u8>> {
    let current = TileInfo::detect(&data).encoding;
    if current == target {
        return Ok(data);
    }
    let plain = match current {
        Encoding::Uncompressed | Encoding::Internal => data,
        Encoding::Gzip => decode_gzip(&data)?,
        Encoding::Zlib => decode_zlib(&data)?,
        Encoding::Brotli | Encoding::Zstd => {
            return Err(MbtError::CannotRecodeCompressedTile(current));
        }
    };
    match target {
        Encoding::Uncompressed => Ok(plain),
        Encoding::Gzip => Ok(encode_gzip(&plain)?),
        other => Err(MbtError::UnsupportedPackTarget(other)),
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::tile_coords;

    #[test]
    fn test_tile_coords() {
        // `{z}/{x}/{y}.{ext}`, with the extension ignored.
        assert_eq!(tile_coords(Path::new("0/0/0.png")), Some((0, 0, 0)));
        assert_eq!(
            tile_coords(Path::new("any/prefix/3/4/5.pbf")),
            Some((3, 4, 5))
        );
        assert_eq!(tile_coords(Path::new("3/4/5")), Some((3, 4, 5)));

        // Non-numeric components are rejected.
        assert_eq!(tile_coords(Path::new("z/4/5.png")), None);
        assert_eq!(tile_coords(Path::new("3/x/5.png")), None);
        assert_eq!(tile_coords(Path::new("3/4/y.png")), None);

        // Zoom must fit in a u8, and there must be enough path components.
        assert_eq!(tile_coords(Path::new("999/4/5.png")), None);
        assert_eq!(tile_coords(Path::new("5.png")), None);
    }
}
