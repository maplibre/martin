#![expect(
    clippy::print_stdout,
    reason = "binary entrypoint writes results to stdout"
)]

use std::io::IsTerminal as _;
use std::path::{Path, PathBuf};

use clap::builder::Styles;
use clap::builder::styling::AnsiColor;
use clap::{Parser, Subcommand, ValueEnum};
use enum_display::EnumDisplay;
use futures::StreamExt as _;
use martin_tile_utils::{Encoding, Format, TileInfo, decode_gzip, decode_zlib, encode_gzip};
use mbtiles::{
    AggHashType, CopyDuplicateMode, CopyType, IntegrityCheckType, MbtResult, MbtType, MbtTypeCli,
    Mbtiles, MbtilesCopier, PatchTypeCli, UpdateZoomType, apply_patch, create_flat_tables,
    create_metadata_table, invert_y_value,
};
use serde::{Deserialize, Serialize};
use tilejson::Bounds;
use tracing::error;
use tracing_subscriber::EnvFilter;
use walkdir::WalkDir;

/// Defines the styles used for the CLI help output.
const HELP_STYLES: Styles = Styles::styled()
    .header(AnsiColor::Blue.on_default().bold())
    .usage(AnsiColor::Blue.on_default().bold())
    .literal(AnsiColor::White.on_default())
    .placeholder(AnsiColor::Green.on_default());

#[derive(Parser, PartialEq, Debug)]
#[command(
    version,
    name = "mbtiles",
    about = "A utility to work with .mbtiles file content",
    after_help = "Use RUST_LOG environment variable to control logging level, e.g. RUST_LOG=debug or RUST_LOG=mbtiles=debug. See https://docs.rs/tracing-subscriber/latest/tracing_subscriber/filter/struct.EnvFilter.html for more information.",
    styles = HELP_STYLES
)]
pub struct Args {
    /// Display detailed information
    #[arg(short, long, hide = true)]
    verbose: bool,
    #[command(subcommand)]
    command: Commands,
}

#[derive(
    Default, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, EnumDisplay, ValueEnum,
)]
#[enum_display(case = "Kebab")]
pub enum OutputFormat {
    #[default]
    Text,
    Json,
    #[value(alias("pretty-json"))]
    JsonPretty,
}

#[derive(Subcommand, PartialEq, Debug)]
enum Commands {
    /// Show `MBTiles` file summary statistics
    #[command(name = "summary", alias = "info")]
    Summary {
        file: PathBuf,
        #[arg(short, long, value_enum, default_value_t=OutputFormat::default())]
        format: OutputFormat,
    },
    /// Prints all values in the metadata table in a free-style, unstable YAML format
    #[command(name = "meta-all")]
    MetaAll {
        /// `MBTiles` file to read from
        file: PathBuf,
    },
    /// Gets a single value from the `MBTiles` metadata table.
    #[command(name = "meta-get", alias = "get-meta")]
    MetaGetValue {
        /// `MBTiles` file to read a value from
        file: PathBuf,
        /// Value to read
        key: String,
    },
    /// Sets a single value in the `MBTiles` metadata table or deletes it if no value.
    #[command(name = "meta-set", alias = "set-meta")]
    MetaSetValue {
        /// `MBTiles` file to modify
        file: PathBuf,
        /// Key to set
        key: String,
        /// Value to set, or nothing if the key should be deleted.
        value: Option<String>,
    },
    /// Compare two files A and B, and generate a new diff file. If the diff file is applied to A, it will produce B.
    #[command(name = "diff")]
    Diff(DiffArgs),
    /// Copy tiles from one mbtiles file to another.
    #[command(name = "copy", alias = "cp")]
    Copy(CopyArgs),
    /// Apply diff file generated from 'copy' command
    #[command(name = "apply-patch", alias = "apply-diff")]
    ApplyPatch {
        /// `MBTiles` file to apply diff to
        base_file: PathBuf,
        /// Diff file
        patch_file: PathBuf,
        /// Force patching operation, ignoring some warnings that otherwise would prevent the operation. Use with caution.
        #[arg(short, long)]
        force: bool,
    },
    /// Update metadata to match the content of the file
    #[command(name = "meta-update", alias = "update-meta")]
    UpdateMetadata {
        /// `MBTiles` file to validate
        file: PathBuf,
        /// Update the min and max zoom levels in the metadata table to match the tiles table.
        #[arg(long, value_enum, default_value_t=UpdateZoomType::default())]
        update_zoom: UpdateZoomType,
    },
    /// Validate tile data if hash of tile data exists in file
    #[command(name = "validate", alias = "check", alias = "verify")]
    Validate {
        /// `MBTiles` file to validate
        file: PathBuf,
        /// Value to specify the extent of the `SQLite` integrity check performed
        #[arg(long, value_enum, default_value_t=IntegrityCheckType::default())]
        integrity_check: IntegrityCheckType,
        /// Update `agg_tiles_hash` metadata value instead of using it to validate if the entire tile store is valid.
        #[arg(long, hide = true)]
        update_agg_tiles_hash: bool,
        /// How should the aggregate tiles hash be checked or updated.
        #[arg(long, value_enum)]
        agg_hash: Option<AggHashType>,
    },
    /// Pack a directory tree of tiles into an `MBTiles` file
    #[command(name = "pack")]
    Pack {
        /// directory to read
        input_directory: PathBuf,
        /// `MBTiles` file to write
        output_file: PathBuf,
        /// Tile ID scheme for input directory
        #[arg(long, value_enum, default_value = "xyz")]
        scheme: TileScheme,
        /// Compression to store tiles with
        #[arg(long, value_enum, default_value = "auto")]
        compress: Compression,
    },
    /// Unpack an `MBTiles` file into a directory tree of tiles
    #[command(name = "unpack")]
    Unpack {
        /// `MBTiles` file to read
        input_file: PathBuf,
        /// directory to write
        output_directory: PathBuf,
        /// Tile ID scheme for output directory
        #[arg(long, value_enum, default_value = "xyz")]
        scheme: TileScheme,
    },
}

#[derive(Clone, Copy, PartialEq, Debug, clap::ValueEnum)]
enum TileScheme {
    /// XYZ (aka. "slippy map") scheme where Y=0 is at the top
    #[value(name = "xyz")]
    Xyz,
    /// TMS scheme where Y=0 is at the bottom
    #[value(name = "tms")]
    Tms,
}

#[derive(Clone, Copy, PartialEq, Debug, clap::ValueEnum)]
enum Compression {
    /// Gzip vector tiles and store everything else as-is, matching `MBTiles` conventions
    #[value(name = "auto")]
    Auto,
    /// Store every tile uncompressed
    #[value(name = "none")]
    None,
    /// Gzip-compress every tile
    #[value(name = "gzip", alias = "gz")]
    Gzip,
}

#[derive(Clone, Default, PartialEq, Debug, clap::Args)]
pub struct CopyArgs {
    /// `MBTiles` file to read from
    src_file: PathBuf,
    /// `MBTiles` file to write to
    dst_file: PathBuf,
    #[command(flatten)]
    pub options: SharedCopyOpts,
    /// Compare source file with this file, and only copy non-identical tiles to destination.
    /// Use `mbtiles diff` as a more convenient way to generate this file.
    /// Use `mbtiles apply-patch` or `mbtiles copy --apply-patch` to apply the diff file.
    #[arg(long, conflicts_with("apply_patch"))]
    diff_with_file: Option<PathBuf>,
    /// Apply a patch file while copying src to dst.
    /// Use `mbtiles diff` or `mbtiles copy --diff-with-file` to generate the patch file.
    /// Use `mbtiles apply-patch` to apply the patch file in-place, without making a copy of the original.
    #[arg(long, conflicts_with("diff_with_file"))]
    apply_patch: Option<PathBuf>,
    /// Specify the type of patch file to generate.
    #[arg(long, requires("diff_with_file"), default_value_t=PatchTypeCli::default())]
    patch_type: PatchTypeCli,
}

#[derive(Clone, Default, PartialEq, Debug, clap::Args)]
pub struct DiffArgs {
    /// First `MBTiles` file to compare
    file1: PathBuf,
    /// Second `MBTiles` file to compare
    file2: PathBuf,
    /// Output file to write the resulting difference to
    diff: PathBuf,
    /// Specify the type of patch file to generate.
    #[arg(long, default_value_t=PatchTypeCli::default())]
    patch_type: PatchTypeCli,

    #[command(flatten)]
    pub options: SharedCopyOpts,
}

#[expect(
    clippy::doc_markdown,
    reason = "for command line arguments, formatting `TileJSON` is awkward"
)]
#[derive(Clone, Default, PartialEq, Debug, clap::Args)]
#[expect(clippy::struct_excessive_bools, reason = "CLI interface")]
pub struct SharedCopyOpts {
    /// Limit what gets copied.
    /// When copying tiles only, the agg_tiles_hash will still be updated unless --skip-agg-tiles-hash is set.
    #[arg(long, value_name = "TYPE", default_value_t=CopyType::default())]
    copy: CopyType,
    /// Use `SQLite` `STRICT` tables when creating a new destination file.
    #[arg(long)]
    strict: bool,
    /// Output format of the destination file, ignored if the file exists. If not specified, defaults to the type of source
    #[arg(long, alias = "dst-type", alias = "dst_type", value_name = "SCHEMA")]
    mbtiles_type: Option<MbtTypeCli>,
    /// Allow copying to existing files, and indicate what to do if a tile with the same Z/X/Y already exists
    #[arg(long, value_enum)]
    on_duplicate: Option<CopyDuplicateMode>,
    /// Minimum zoom level to copy
    #[arg(long, conflicts_with("zoom_levels"))]
    min_zoom: Option<u8>,
    /// Maximum zoom level to copy
    #[arg(long, conflicts_with("zoom_levels"))]
    max_zoom: Option<u8>,
    /// List of zoom levels to copy
    #[arg(long, value_delimiter = ',')]
    zoom_levels: Vec<u8>,
    /// Bounding box to copy, in the format `min_lon,min_lat,max_lon,max_lat`. Can be used multiple times.
    #[arg(long)]
    bbox: Vec<Bounds>,
    /// Skip generating a global hash for mbtiles validation. By default, `mbtiles` will compute `agg_tiles_hash` metadata value.
    #[arg(long)]
    skip_agg_tiles_hash: bool,
    /// Force copy operation, ignoring some warnings that otherwise would prevent the operation. Use with caution.
    #[arg(short, long)]
    force: bool,
    /// Perform agg_hash validation on the original and destination files.
    #[arg(long)]
    validate: bool,
}

impl SharedCopyOpts {
    #[must_use]
    pub fn into_copier(
        self,
        src_file: PathBuf,
        dst_file: PathBuf,
        diff_with_file: Option<PathBuf>,
        apply_patch: Option<PathBuf>,
        patch_type: PatchTypeCli,
    ) -> MbtilesCopier {
        MbtilesCopier {
            src_file,
            dst_file,
            diff_with_file: diff_with_file.map(|p| (p, patch_type.into())),
            apply_patch,
            // Shared
            copy: self.copy,
            dst_type_cli: self.mbtiles_type,
            on_duplicate: self.on_duplicate,
            min_zoom: self.min_zoom,
            max_zoom: self.max_zoom,
            zoom_levels: self.zoom_levels,
            bbox: self.bbox,
            skip_agg_tiles_hash: self.skip_agg_tiles_hash,
            force: self.force,
            validate: self.validate,
            strict: self.strict,
            // Constants
            dst_type: None, // Taken from dst_type_cli
        }
    }
}

#[tokio::main]
async fn main() {
    let env_filter = EnvFilter::builder()
        .with_default_directive("mbtiles=info".parse().expect("valid default directive"))
        .from_env_lossy();
    tracing_subscriber::fmt()
        .compact()
        .without_time()
        .with_target(false)
        .with_ansi(std::io::stderr().is_terminal())
        .with_writer(std::io::stderr)
        .with_env_filter(env_filter)
        .init();

    if let Err(err) = main_int().await {
        error!("{err}");
        std::process::exit(1);
    }
}

async fn main_int() -> anyhow::Result<()> {
    let args = Args::parse();
    match args.command {
        Commands::MetaAll { file } => {
            meta_print_all(file.as_path()).await?;
        }
        Commands::MetaGetValue { file, key } => {
            meta_get_value(file.as_path(), &key).await?;
        }
        Commands::MetaSetValue { file, key, value } => {
            meta_set_value(file.as_path(), &key, value.as_deref()).await?;
        }
        Commands::Copy(args) => {
            let copier = args.options.into_copier(
                args.src_file,
                args.dst_file,
                args.diff_with_file,
                args.apply_patch,
                args.patch_type,
            );
            copier.run().await?;
        }
        Commands::Diff(args) => {
            let copier = args.options.into_copier(
                args.file1,
                args.diff,
                Some(args.file2),
                None,
                args.patch_type,
            );
            copier.run().await?;
        }
        Commands::ApplyPatch {
            base_file,
            patch_file,
            force,
        } => {
            apply_patch(base_file, patch_file, force).await?;
        }
        Commands::UpdateMetadata { file, update_zoom } => {
            let mbt = Mbtiles::new(file.as_path())?;
            let mut conn = mbt.open().await?;
            mbt.update_metadata(&mut conn, update_zoom).await?;
        }
        Commands::Validate {
            file,
            integrity_check,
            update_agg_tiles_hash,
            agg_hash,
        } => {
            if update_agg_tiles_hash && agg_hash.is_some() {
                anyhow::bail!("Cannot use both --agg-hash and --update-agg-tiles-hash");
            }
            let agg_hash = agg_hash.unwrap_or_else(|| {
                if update_agg_tiles_hash {
                    AggHashType::Update
                } else {
                    AggHashType::default()
                }
            });
            let mbt = Mbtiles::new(file.as_path())?;
            mbt.open_and_validate(integrity_check, agg_hash).await?;
        }
        Commands::Summary { file, format } => {
            let mbt = Mbtiles::new(file.as_path())?;
            let mut conn = mbt.open_readonly().await?;
            let summary = mbt.summary(&mut conn).await?;
            match format {
                OutputFormat::Text => println!("{summary}"),
                OutputFormat::Json => println!("{}", serde_json::to_string(&summary)?),
                OutputFormat::JsonPretty => println!("{}", serde_json::to_string_pretty(&summary)?),
            }
        }
        Commands::Pack {
            input_directory,
            output_file,
            scheme,
            compress,
        } => {
            pack(&input_directory, &output_file, scheme, compress).await?;
        }
        Commands::Unpack {
            input_file,
            output_directory,
            scheme,
        } => {
            unpack(&input_file, &output_directory, scheme).await?;
        }
    }

    Ok(())
}

async fn meta_print_all(file: &Path) -> anyhow::Result<()> {
    let mbt = Mbtiles::new(file)?;
    let mut conn = mbt.open_readonly().await?;
    let metadata = mbt.get_metadata(&mut conn).await?;
    print!("{}", serde_saphyr::to_string(&metadata)?);
    let tile_info = mbt.detect_format(&metadata.tilejson, &mut conn).await?;
    // For compatibility, pretend tile_info is part of metadata YAML output
    if let Some(tile_info) = tile_info {
        let encoding = tile_info.encoding.compression().unwrap_or("''");
        println!("tile_info:");
        println!("  format: {}", tile_info.format);
        println!("  encoding: {encoding}");
    } else {
        println!("tile_info: null");
    }
    Ok(())
}

async fn meta_get_value(file: &Path, key: &str) -> MbtResult<()> {
    let mbt = Mbtiles::new(file)?;
    let mut conn = mbt.open_readonly().await?;
    if let Some(s) = mbt.get_metadata_value(&mut conn, key).await? {
        println!("{s}");
    }
    Ok(())
}

async fn meta_set_value(file: &Path, key: &str, value: Option<&str>) -> MbtResult<()> {
    let mbt = Mbtiles::new(file)?;
    let mut conn = mbt.open().await?;
    if let Some(value) = value {
        mbt.set_metadata_value(&mut conn, key, value).await
    } else {
        mbt.delete_metadata_value(&mut conn, key).await
    }
}

/// Number of tiles inserted per transaction while packing.
const PACK_BATCH_SIZE: usize = 1000;

/// Extracts the `(z, x, y)` coordinates from a `{z}/{x}/{y}.{ext}` tile path.
/// `y` is taken from the file stem so the extension is ignored.
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
fn recode_tile(data: Vec<u8>, target: Encoding) -> anyhow::Result<Vec<u8>> {
    let current = TileInfo::detect(&data).encoding;
    if current == target {
        return Ok(data);
    }
    let plain = match current {
        Encoding::Uncompressed | Encoding::Internal => data,
        Encoding::Gzip => decode_gzip(&data)?,
        Encoding::Zlib => decode_zlib(&data)?,
        Encoding::Brotli | Encoding::Zstd => {
            anyhow::bail!("Cannot re-encode {current:?}-compressed tile data");
        }
    };
    match target {
        Encoding::Uncompressed => Ok(plain),
        Encoding::Gzip => Ok(encode_gzip(&plain)?),
        other => anyhow::bail!("Unsupported pack compression target: {other:?}"),
    }
}

async fn pack(
    input_directory: &Path,
    output_file: &Path,
    scheme: TileScheme,
    compress: Compression,
) -> anyhow::Result<()> {
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
        let entry = entry?;
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
            .ok_or_else(|| anyhow::anyhow!("Unsupported file extension: {}", path.display()))?;
        match format {
            None => format = Some(detected),
            Some(f) if f != detected => {
                anyhow::bail!(
                    "Inconsistent tile formats: found {detected} at {} but earlier tiles were {f}",
                    path.display()
                );
            }
            Some(_) => {}
        }

        let data = std::fs::read(path)?;
        // `auto` follows the MBTiles convention of gzipping vector tiles and leaving
        // raster tiles untouched; explicit choices apply to every tile.
        let target = match compress {
            Compression::Auto if detected == Format::Mvt => Encoding::Gzip,
            Compression::Auto | Compression::None => Encoding::Uncompressed,
            Compression::Gzip => Encoding::Gzip,
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

async fn unpack(
    input_file: &Path,
    output_directory: &Path,
    scheme: TileScheme,
) -> anyhow::Result<()> {
    if !input_file.exists() {
        anyhow::bail!("Input file does not exist: {}", input_file.display());
    }

    let mbt = Mbtiles::new(input_file)?;
    let mut conn = mbt.open_readonly().await?;

    // Derive the output file extension from the stored format.
    let format = mbt.get_metadata_value(&mut conn, "format").await?;
    let Some(format_str) = format.as_deref() else {
        anyhow::bail!("No format specified in MBTiles metadata");
    };
    let extension = Format::parse(format_str)
        .ok_or_else(|| anyhow::anyhow!("Unknown format in MBTiles metadata: {format_str}"))?
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

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use clap::Parser as _;
    use clap::error::ErrorKind;
    use mbtiles::CopyDuplicateMode;

    use super::*;
    use crate::Commands::{
        ApplyPatch, Copy, Diff, MetaGetValue, MetaSetValue, Pack, Unpack, Validate,
    };
    use crate::{Args, IntegrityCheckType};

    #[test]
    fn test_copy_no_arguments() {
        assert_eq!(
            Args::try_parse_from(["mbtiles", "copy"])
                .unwrap_err()
                .kind(),
            ErrorKind::MissingRequiredArgument
        );
    }

    #[test]
    fn test_copy_minimal_arguments() {
        assert_eq!(
            Args::parse_from(["mbtiles", "copy", "src_file", "dst_file"]),
            Args {
                verbose: false,
                command: Copy(CopyArgs {
                    src_file: PathBuf::from("src_file"),
                    dst_file: PathBuf::from("dst_file"),
                    ..Default::default()
                })
            }
        );
    }

    #[test]
    fn test_copy_min_max_zoom_arguments() {
        let args = Args::parse_from([
            "mbtiles",
            "copy",
            "src_file",
            "dst_file",
            "--max-zoom",
            "100",
            "--min-zoom",
            "1",
        ]);
        assert_eq!(
            args,
            Args {
                verbose: false,
                command: Copy(CopyArgs {
                    src_file: PathBuf::from("src_file"),
                    dst_file: PathBuf::from("dst_file"),
                    options: SharedCopyOpts {
                        min_zoom: Some(1),
                        max_zoom: Some(100),
                        ..Default::default()
                    },
                    ..Default::default()
                })
            }
        );
    }

    #[test]
    fn test_copy_strict_argument() {
        assert_eq!(
            Args::parse_from(["mbtiles", "copy", "src_file", "dst_file", "--strict"]),
            Args {
                verbose: false,
                command: Copy(CopyArgs {
                    src_file: PathBuf::from("src_file"),
                    dst_file: PathBuf::from("dst_file"),
                    options: SharedCopyOpts {
                        strict: true,
                        ..Default::default()
                    },
                    ..Default::default()
                })
            }
        );
    }

    #[test]
    fn test_copy_min_max_zoom_no_arguments() {
        assert_eq!(
            Args::try_parse_from([
                "mbtiles",
                "copy",
                "src_file",
                "dst_file",
                "--max-zoom",
                "--min-zoom",
            ])
            .unwrap_err()
            .kind(),
            ErrorKind::InvalidValue
        );
    }

    #[test]
    fn test_copy_min_max_zoom_with_zoom_levels_arguments() {
        assert_eq!(
            Args::try_parse_from([
                "mbtiles",
                "copy",
                "src_file",
                "dst_file",
                "--max-zoom",
                "100",
                "--min-zoom",
                "1",
                "--zoom-levels",
                "3,7,1"
            ])
            .unwrap_err()
            .kind(),
            ErrorKind::ArgumentConflict
        );
    }

    #[test]
    fn test_copy_zoom_levels_arguments() {
        assert_eq!(
            Args::parse_from([
                "mbtiles",
                "copy",
                "src_file",
                "dst_file",
                "--zoom-levels",
                "3,7,1"
            ]),
            Args {
                verbose: false,
                command: Copy(CopyArgs {
                    src_file: PathBuf::from("src_file"),
                    dst_file: PathBuf::from("dst_file"),
                    options: SharedCopyOpts {
                        zoom_levels: vec![3, 7, 1],
                        ..Default::default()
                    },
                    ..Default::default()
                })
            }
        );
    }

    #[test]
    fn test_copy_diff_with_file_arguments() {
        assert_eq!(
            Args::parse_from([
                "mbtiles",
                "copy",
                "src_file",
                "dst_file",
                "--diff-with-file",
                "no_file",
            ]),
            Args {
                verbose: false,
                command: Copy(CopyArgs {
                    src_file: PathBuf::from("src_file"),
                    dst_file: PathBuf::from("dst_file"),
                    diff_with_file: Some(PathBuf::from("no_file")),
                    ..Default::default()
                })
            }
        );
    }

    #[test]
    fn test_copy_diff_with_override_copy_duplicate_mode() {
        assert_eq!(
            Args::parse_from([
                "mbtiles",
                "copy",
                "src_file",
                "dst_file",
                "--on-duplicate",
                "override"
            ]),
            Args {
                verbose: false,
                command: Copy(CopyArgs {
                    src_file: PathBuf::from("src_file"),
                    dst_file: PathBuf::from("dst_file"),
                    options: SharedCopyOpts {
                        on_duplicate: Some(CopyDuplicateMode::Override),
                        ..Default::default()
                    },
                    ..Default::default()
                })
            }
        );
    }

    #[test]
    fn test_copy_limit() {
        assert_eq!(
            Args::parse_from([
                "mbtiles", "copy", "src_file", "dst_file", "--copy", "metadata"
            ]),
            Args {
                verbose: false,
                command: Copy(CopyArgs {
                    src_file: PathBuf::from("src_file"),
                    dst_file: PathBuf::from("dst_file"),
                    options: SharedCopyOpts {
                        copy: CopyType::Metadata,
                        ..Default::default()
                    },
                    ..Default::default()
                })
            }
        );
    }

    #[test]
    fn test_diff() {
        assert_eq!(
            Args::parse_from([
                "mbtiles",
                "diff",
                "file1.mbtiles",
                "file2.mbtiles",
                "../delta.mbtiles",
                "--on-duplicate",
                "override"
            ]),
            Args {
                verbose: false,
                command: Diff(DiffArgs {
                    file1: PathBuf::from("file1.mbtiles"),
                    file2: PathBuf::from("file2.mbtiles"),
                    diff: PathBuf::from("../delta.mbtiles"),
                    patch_type: PatchTypeCli::Whole,
                    options: SharedCopyOpts {
                        on_duplicate: Some(CopyDuplicateMode::Override),
                        ..Default::default()
                    },
                })
            }
        );
    }

    #[test]
    fn test_meta_get_no_arguments() {
        assert_eq!(
            Args::try_parse_from(["mbtiles", "meta-get"])
                .unwrap_err()
                .kind(),
            ErrorKind::MissingRequiredArgument
        );
    }

    #[test]
    fn test_meta_get_with_arguments() {
        assert_eq!(
            Args::parse_from(["mbtiles", "meta-get", "src_file", "key"]),
            Args {
                verbose: false,
                command: MetaGetValue {
                    file: PathBuf::from("src_file"),
                    key: "key".to_string(),
                }
            }
        );
    }

    #[test]
    fn test_meta_set_no_arguments() {
        assert_eq!(
            Args::try_parse_from(["mbtiles", "meta-get"])
                .unwrap_err()
                .kind(),
            ErrorKind::MissingRequiredArgument
        );
    }

    #[test]
    fn test_meta_set_no_value_argument() {
        assert_eq!(
            Args::parse_from(["mbtiles", "meta-set", "src_file", "key"]),
            Args {
                verbose: false,
                command: MetaSetValue {
                    file: PathBuf::from("src_file"),
                    key: "key".to_string(),
                    value: None
                }
            }
        );
    }

    #[test]
    fn test_meta_get_with_all_arguments() {
        assert_eq!(
            Args::parse_from(["mbtiles", "meta-set", "src_file", "key", "value"]),
            Args {
                verbose: false,
                command: MetaSetValue {
                    file: PathBuf::from("src_file"),
                    key: "key".to_string(),
                    value: Some("value".to_string())
                }
            }
        );
    }

    #[test]
    fn test_apply_diff_with_arguments() {
        assert_eq!(
            Args::parse_from(["mbtiles", "apply-diff", "src_file", "diff_file"]),
            Args {
                verbose: false,
                command: ApplyPatch {
                    base_file: PathBuf::from("src_file"),
                    patch_file: PathBuf::from("diff_file"),
                    force: false,
                }
            }
        );
    }

    #[test]
    fn test_validate() {
        assert_eq!(
            Args::parse_from(["mbtiles", "validate", "src_file", "--agg-hash", "off"]),
            Args {
                verbose: false,
                command: Validate {
                    file: PathBuf::from("src_file"),
                    integrity_check: IntegrityCheckType::Quick,
                    update_agg_tiles_hash: false,
                    agg_hash: Some(AggHashType::Off),
                }
            }
        );
    }

    // Behavioural pack/unpack coverage (round-trips, scheme flips, compression, metadata,
    // and CLI error paths) lives in the integration suite `tests/test.sh`, which drives the
    // real binary against fixture MBTiles. The unit tests below stay pure: argument parsing
    // and the `tile_coords` path parser, neither of which touches the filesystem or SQLite.

    #[test]
    fn test_pack_defaults() {
        assert_eq!(
            Args::parse_from(["mbtiles", "pack", "src_dir", "out.mbtiles"]),
            Args {
                verbose: false,
                command: Pack {
                    input_directory: PathBuf::from("src_dir"),
                    output_file: PathBuf::from("out.mbtiles"),
                    scheme: TileScheme::Xyz,
                    compress: Compression::Auto,
                }
            }
        );
    }

    #[test]
    fn test_pack_tms_uncompressed() {
        assert_eq!(
            Args::parse_from([
                "mbtiles",
                "pack",
                "src_dir",
                "out.mbtiles",
                "--scheme",
                "tms",
                "--compress",
                "none",
            ]),
            Args {
                verbose: false,
                command: Pack {
                    input_directory: PathBuf::from("src_dir"),
                    output_file: PathBuf::from("out.mbtiles"),
                    scheme: TileScheme::Tms,
                    compress: Compression::None,
                }
            }
        );
    }

    #[test]
    fn test_pack_compress_gzip_alias() {
        let Pack { compress, .. } =
            Args::parse_from(["mbtiles", "pack", "src", "out.mbtiles", "--compress", "gz"]).command
        else {
            panic!("expected a pack command");
        };
        assert_eq!(compress, Compression::Gzip);
    }

    #[test]
    fn test_unpack_scheme() {
        assert_eq!(
            Args::parse_from([
                "mbtiles",
                "unpack",
                "in.mbtiles",
                "out_dir",
                "--scheme",
                "tms"
            ]),
            Args {
                verbose: false,
                command: Unpack {
                    input_file: PathBuf::from("in.mbtiles"),
                    output_directory: PathBuf::from("out_dir"),
                    scheme: TileScheme::Tms,
                }
            }
        );
    }

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
