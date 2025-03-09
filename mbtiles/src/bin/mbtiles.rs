use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};
use martin_observability_utils::{LogFormat, LogLevel, MartinObservability};
use mbtiles::{
    apply_patch, AggHashType, CopyDuplicateMode, CopyType, IntegrityCheckType, MbtResult,
    MbtTypeCli, Mbtiles, MbtilesCopier, PatchTypeCli, UpdateZoomType,
};
use tilejson::Bounds;
use tracing::error;

#[derive(Parser, PartialEq, Debug)]
#[command(
    version,
    name = "mbtiles",
    about = "A utility to work with .mbtiles file content",
    after_help = "Use RUST_LOG environment variable to control logging level, e.g. RUST_LOG=debug or RUST_LOG=mbtiles=debug. See https://docs.rs/tracing-subscriber/latest/tracing_subscriber/filter/struct.EnvFilter.html#example-syntax for more information."
)]
pub struct Args {
    /// Display detailed information
    #[arg(short, long, hide = true)]
    verbose: bool,
    #[command(subcommand)]
    command: Commands,
}

#[allow(clippy::doc_markdown)]
#[derive(Subcommand, PartialEq, Debug)]
enum Commands {
    /// Show MBTiles file summary statistics
    #[command(name = "summary", alias = "info")]
    Summary { file: PathBuf },
    /// Prints all values in the metadata table in a free-style, unstable YAML format
    #[command(name = "meta-all")]
    MetaAll {
        /// MBTiles file to read from
        file: PathBuf,
    },
    /// Gets a single value from the MBTiles metadata table.
    #[command(name = "meta-get", alias = "get-meta")]
    MetaGetValue {
        /// MBTiles file to read a value from
        file: PathBuf,
        /// Value to read
        key: String,
    },
    /// Sets a single value in the MBTiles metadata table or deletes it if no value.
    #[command(name = "meta-set", alias = "set-meta")]
    MetaSetValue {
        /// MBTiles file to modify
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
        /// MBTiles file to apply diff to
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
        /// MBTiles file to validate
        file: PathBuf,
        /// Update the min and max zoom levels in the metadata table to match the tiles table.
        #[arg(long, value_enum, default_value_t=UpdateZoomType::default())]
        update_zoom: UpdateZoomType,
    },
    /// Validate tile data if hash of tile data exists in file
    #[command(name = "validate", alias = "check", alias = "verify")]
    Validate {
        /// MBTiles file to validate
        file: PathBuf,
        /// Value to specify the extent of the SQLite integrity check performed
        #[arg(long, value_enum, default_value_t=IntegrityCheckType::default())]
        integrity_check: IntegrityCheckType,
        /// Update `agg_tiles_hash` metadata value instead of using it to validate if the entire tile store is valid.
        #[arg(long, hide = true)]
        update_agg_tiles_hash: bool,
        /// How should the aggregate tiles hash be checked or updated.
        #[arg(long, value_enum)]
        agg_hash: Option<AggHashType>,
    },
}

#[allow(clippy::doc_markdown)]
#[derive(Clone, Default, PartialEq, Debug, clap::Args)]
pub struct CopyArgs {
    /// MBTiles file to read from
    src_file: PathBuf,
    /// MBTiles file to write to
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

#[allow(clippy::doc_markdown)]
#[derive(Clone, Default, PartialEq, Debug, clap::Args)]
pub struct DiffArgs {
    /// First MBTiles file to compare
    file1: PathBuf,
    /// Second MBTiles file to compare
    file2: PathBuf,
    /// Output file to write the resulting difference to
    diff: PathBuf,
    /// Specify the type of patch file to generate.
    #[arg(long, default_value_t=PatchTypeCli::default())]
    patch_type: PatchTypeCli,

    #[command(flatten)]
    pub options: SharedCopyOpts,
}

#[allow(clippy::doc_markdown)]
#[derive(Clone, Default, PartialEq, Debug, clap::Args)]
pub struct SharedCopyOpts {
    /// Limit what gets copied.
    /// When copying tiles only, the agg_tiles_hash will still be updated unless --skip-agg-tiles-hash is set.
    #[arg(long, value_name = "TYPE", default_value_t=CopyType::default())]
    copy: CopyType,
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
            // Constants
            dst_type: None, // Taken from dst_type_cli
        }
    }
}

#[tokio::main]
async fn main() {
    // since logging is not yet available, we have to manually check the locations
    let log_filter = LogLevel::from_argument("--log-level")
        .or_env_var("MBTILES_LOG_FORMAT")
        .lossy_parse_to_filter_with_default("martin=info");
    let log_format = LogFormat::from_argument("--log-level")
        .or_env_var("RUST_LOG")
        .or_default(LogFormatOptions::Compact);
    MartinObservability::from((log_filter, log_format))
        .with_initialised_log_tracing()
        .set_global_subscriber();

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
        Commands::Summary { file } => {
            let mbt = Mbtiles::new(file.as_path())?;
            let mut conn = mbt.open_readonly().await?;
            println!("MBTiles file summary for {mbt}");
            println!("{}", mbt.summary(&mut conn).await?);
        }
    }

    Ok(())
}

async fn meta_print_all(file: &Path) -> anyhow::Result<()> {
    let mbt = Mbtiles::new(file)?;
    let mut conn = mbt.open_readonly().await?;
    let metadata = mbt.get_metadata(&mut conn).await?;
    println!("{}", serde_yaml::to_string(&metadata)?);
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use clap::error::ErrorKind;
    use clap::Parser;
    use mbtiles::CopyDuplicateMode;

    use super::*;
    use crate::Commands::{ApplyPatch, Copy, Diff, MetaGetValue, MetaSetValue, Validate};
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
            Args::parse_from(["mbtiles", "copy", "src_file", "dst_file", "--copy", "metadata"]),
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
}
