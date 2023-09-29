use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};
use log::{error, LevelFilter};
use martin_mbtiles::{
    apply_mbtiles_diff, IntegrityCheckType, MbtResult, Mbtiles, TileCopierOptions,
};

#[derive(Parser, PartialEq, Eq, Debug)]
#[command(
    version,
    name = "mbtiles",
    about = "A utility to work with .mbtiles file content"
)]
pub struct Args {
    /// Display detailed information
    #[arg(short, long, hide = true)]
    verbose: bool,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, PartialEq, Eq, Debug)]
enum Commands {
    /// Prints all values in the metadata table in a free-style, unstable YAML format
    #[command(name = "meta-all")]
    MetaAll {
        /// MBTiles file to read from
        file: PathBuf,
    },
    /// Gets a single value from the MBTiles metadata table.
    #[command(name = "meta-get")]
    MetaGetValue {
        /// MBTiles file to read a value from
        file: PathBuf,
        /// Value to read
        key: String,
    },
    /// Sets a single value in the MBTiles' file metadata table or deletes it if no value.
    #[command(name = "meta-set")]
    MetaSetValue {
        /// MBTiles file to modify
        file: PathBuf,
        /// Key to set
        key: String,
        /// Value to set, or nothing if the key should be deleted.
        value: Option<String>,
    },
    /// Copy tiles from one mbtiles file to another.
    #[command(name = "copy")]
    Copy(TileCopierOptions),
    /// Apply diff file generated from 'copy' command
    #[command(name = "apply-diff")]
    ApplyDiff {
        /// MBTiles file to apply diff to
        src_file: PathBuf,
        /// Diff file
        diff_file: PathBuf,
    },
    /// Validate tile data if hash of tile data exists in file
    #[command(name = "validate")]
    Validate {
        /// MBTiles file to validate
        file: PathBuf,
        /// Value to specify the extent of the SQLite integrity check performed
        #[arg(long, value_enum, default_value_t=IntegrityCheckType::default())]
        integrity_check: IntegrityCheckType,
        /// Update `agg_tiles_hash` metadata value instead of using it to validate if the entire tile store is valid.
        #[arg(long)]
        update_agg_tiles_hash: bool,
    },
}

#[tokio::main]
async fn main() {
    env_logger::builder()
        .filter_level(LevelFilter::Info)
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
            meta_set_value(file.as_path(), &key, value).await?;
        }
        Commands::Copy(opts) => {
            opts.run().await?;
        }
        Commands::ApplyDiff {
            src_file,
            diff_file,
        } => {
            apply_mbtiles_diff(src_file, diff_file).await?;
        }
        Commands::Validate {
            file,
            integrity_check,
            update_agg_tiles_hash,
        } => {
            validate_mbtiles(file.as_path(), integrity_check, update_agg_tiles_hash).await?;
        }
    }

    Ok(())
}

async fn meta_print_all(file: &Path) -> anyhow::Result<()> {
    let mbt = Mbtiles::new(file)?;
    let mut conn = mbt.open_with_hashes(true).await?;
    let metadata = mbt.get_metadata(&mut conn).await?;
    println!("{}", serde_yaml::to_string(&metadata)?);
    Ok(())
}

async fn meta_get_value(file: &Path, key: &str) -> MbtResult<()> {
    let mbt = Mbtiles::new(file)?;
    let mut conn = mbt.open_with_hashes(true).await?;
    if let Some(s) = mbt.get_metadata_value(&mut conn, key).await? {
        println!("{s}");
    }
    Ok(())
}

async fn meta_set_value(file: &Path, key: &str, value: Option<String>) -> MbtResult<()> {
    let mbt = Mbtiles::new(file)?;
    let mut conn = mbt.open_with_hashes(false).await?;
    mbt.set_metadata_value(&mut conn, key, value).await
}

async fn validate_mbtiles(
    file: &Path,
    check_type: IntegrityCheckType,
    update_agg_tiles_hash: bool,
) -> MbtResult<()> {
    let mbt = Mbtiles::new(file)?;
    let mut conn = mbt.open_with_hashes(!update_agg_tiles_hash).await?;
    mbt.check_integrity(&mut conn, check_type).await?;
    mbt.check_each_tile_hash(&mut conn).await?;
    if update_agg_tiles_hash {
        mbt.update_agg_tiles_hash(&mut conn).await
    } else {
        mbt.check_agg_tiles_hashes(&mut conn).await
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use clap::error::ErrorKind;
    use clap::Parser;
    use martin_mbtiles::{CopyDuplicateMode, TileCopierOptions};

    use crate::Commands::{ApplyDiff, Copy, MetaGetValue, MetaSetValue, Validate};
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
                command: Copy(TileCopierOptions::new(
                    PathBuf::from("src_file"),
                    PathBuf::from("dst_file")
                ))
            }
        );
    }

    #[test]
    fn test_copy_min_max_zoom_arguments() {
        assert_eq!(
            Args::parse_from([
                "mbtiles",
                "copy",
                "src_file",
                "dst_file",
                "--max-zoom",
                "100",
                "--min-zoom",
                "1"
            ]),
            Args {
                verbose: false,
                command: Copy(
                    TileCopierOptions::new(PathBuf::from("src_file"), PathBuf::from("dst_file"))
                        .min_zoom(Some(1))
                        .max_zoom(Some(100))
                )
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
                command: Copy(
                    TileCopierOptions::new(PathBuf::from("src_file"), PathBuf::from("dst_file"))
                        .zoom_levels(vec![1, 3, 7])
                )
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
                command: Copy(
                    TileCopierOptions::new(PathBuf::from("src_file"), PathBuf::from("dst_file"))
                        .diff_with_file(PathBuf::from("no_file"))
                )
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
                command: Copy(
                    TileCopierOptions::new(PathBuf::from("src_file"), PathBuf::from("dst_file"))
                        .on_duplicate(CopyDuplicateMode::Override)
                )
            }
        );
    }

    #[test]
    fn test_copy_diff_with_ignore_copy_duplicate_mode() {
        assert_eq!(
            Args::parse_from([
                "mbtiles",
                "copy",
                "src_file",
                "dst_file",
                "--on-duplicate",
                "ignore"
            ]),
            Args {
                verbose: false,
                command: Copy(
                    TileCopierOptions::new(PathBuf::from("src_file"), PathBuf::from("dst_file"))
                        .on_duplicate(CopyDuplicateMode::Ignore)
                )
            }
        );
    }

    #[test]
    fn test_copy_diff_with_abort_copy_duplicate_mode() {
        assert_eq!(
            Args::parse_from([
                "mbtiles",
                "copy",
                "src_file",
                "dst_file",
                "--on-duplicate",
                "abort"
            ]),
            Args {
                verbose: false,
                command: Copy(
                    TileCopierOptions::new(PathBuf::from("src_file"), PathBuf::from("dst_file"))
                        .on_duplicate(CopyDuplicateMode::Abort)
                )
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
                command: ApplyDiff {
                    src_file: PathBuf::from("src_file"),
                    diff_file: PathBuf::from("diff_file"),
                }
            }
        );
    }

    #[test]
    fn test_validate() {
        assert_eq!(
            Args::parse_from(["mbtiles", "validate", "src_file"]),
            Args {
                verbose: false,
                command: Validate {
                    file: PathBuf::from("src_file"),
                    integrity_check: IntegrityCheckType::Quick,
                    update_agg_tiles_hash: false
                }
            }
        );
    }
}
