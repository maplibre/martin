use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};
use log::error;
use mbtiles::{
    apply_patch, AggHashType, CopyDuplicateMode, CopyType, IntegrityCheckType, MbtResult,
    MbtTypeCli, Mbtiles, MbtilesCopier,
};
use tilejson::Bounds;

#[derive(Parser, PartialEq, Debug)]
#[command(
    version,
    name = "mbtiles",
    about = "A utility to work with .mbtiles file content",
    after_help = "Use RUST_LOG environment variable to control logging level, e.g. RUST_LOG=debug or RUST_LOG=mbtiles=debug. See https://docs.rs/env_logger/latest/env_logger/index.html#enabling-logging for more information."
)]
pub struct Args {
    /// Display detailed information
    #[arg(short, long, hide = true)]
    verbose: bool,
    #[command(subcommand)]
    command: Commands,
}

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
    /// Copy tiles from one mbtiles file to another.
    #[command(name = "copy", alias = "cp")]
    Copy(CopyArgs),
    /// Apply diff file generated from 'copy' command
    #[command(name = "apply-patch", alias = "apply-diff")]
    ApplyPatch {
        /// MBTiles file to apply diff to
        src_file: PathBuf,
        /// Diff file
        diff_file: PathBuf,
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

#[derive(Clone, Default, PartialEq, Debug, clap::Args)]
pub struct CopyArgs {
    /// MBTiles file to read from
    pub src_file: PathBuf,
    /// MBTiles file to write to
    pub dst_file: PathBuf,
    /// Limit what gets copied
    #[arg(long, value_name = "TYPE", default_value_t=CopyType::default())]
    pub copy: CopyType,
    /// Output format of the destination file, ignored if the file exists. If not specified, defaults to the type of source
    #[arg(long, alias = "dst-type", alias = "dst_type", value_name = "SCHEMA")]
    pub mbtiles_type: Option<MbtTypeCli>,
    /// Allow copying to existing files, and indicate what to do if a tile with the same Z/X/Y already exists
    #[arg(long, value_enum)]
    pub on_duplicate: Option<CopyDuplicateMode>,
    /// Minimum zoom level to copy
    #[arg(long, conflicts_with("zoom_levels"))]
    pub min_zoom: Option<u8>,
    /// Maximum zoom level to copy
    #[arg(long, conflicts_with("zoom_levels"))]
    pub max_zoom: Option<u8>,
    /// List of zoom levels to copy
    #[arg(long, value_delimiter = ',')]
    pub zoom_levels: Vec<u8>,
    /// Bounding box to copy, in the format `min_lon,min_lat,max_lon,max_lat`. Can be used multiple times.
    #[arg(long)]
    pub bbox: Vec<Bounds>,
    /// Compare source file with this file, and only copy non-identical tiles to destination.
    /// It should be later possible to run `mbtiles apply-diff SRC_FILE DST_FILE` to get the same DIFF file.
    #[arg(long, conflicts_with("apply_patch"))]
    pub diff_with_file: Option<PathBuf>,
    /// Compare source file with this file, and only copy non-identical tiles to destination.
    /// It should be later possible to run `mbtiles apply-diff SRC_FILE DST_FILE` to get the same DIFF file.
    #[arg(long, conflicts_with("diff_with_file"))]
    pub apply_patch: Option<PathBuf>,
    /// Skip generating a global hash for mbtiles validation. By default, `mbtiles` will compute `agg_tiles_hash` metadata value.
    #[arg(long)]
    pub skip_agg_tiles_hash: bool,
}

#[tokio::main]
async fn main() {
    let env = env_logger::Env::default().default_filter_or("mbtiles=info");
    env_logger::Builder::from_env(env)
        .format_indent(None)
        .format_module_path(false)
        .format_target(false)
        .format_timestamp(None)
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
        Commands::Copy(opts) => {
            let opts = MbtilesCopier {
                src_file: opts.src_file,
                dst_file: opts.dst_file,
                copy: opts.copy,
                dst_type_cli: opts.mbtiles_type,
                dst_type: None,
                on_duplicate: opts.on_duplicate,
                min_zoom: opts.min_zoom,
                max_zoom: opts.max_zoom,
                zoom_levels: opts.zoom_levels,
                bbox: opts.bbox,
                diff_with_file: opts.diff_with_file,
                apply_patch: opts.apply_patch,
                skip_agg_tiles_hash: opts.skip_agg_tiles_hash,
            };
            opts.run().await?;
        }
        Commands::ApplyPatch {
            src_file,
            diff_file,
        } => {
            apply_patch(src_file, diff_file).await?;
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
            mbt.validate(integrity_check, agg_hash).await?;
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
    use crate::Commands::{ApplyPatch, Copy, MetaGetValue, MetaSetValue, Validate};
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
                    min_zoom: Some(1),
                    max_zoom: Some(100),
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
                    zoom_levels: vec![3, 7, 1],
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
                    on_duplicate: Some(CopyDuplicateMode::Override),
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
                    copy: CopyType::Metadata,
                    ..Default::default()
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
                    src_file: PathBuf::from("src_file"),
                    diff_file: PathBuf::from("diff_file"),
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
