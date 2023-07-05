use std::path::{Path, PathBuf};

use anyhow::Result;
use clap::{Parser, Subcommand};
use martin_mbtiles::{apply_mbtiles_diff, copy_mbtiles_file, Mbtiles, TileCopierOptions};
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{Connection, SqliteConnection};

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
    // /// Prints all values in the metadata table.
    // #[command(name = "meta-all")]
    // MetaAll {
    //     /// MBTiles file to read from
    //     file: PathBuf,
    // },
    /// Gets a single value from the MBTiles metadata table.
    #[command(name = "meta-get")]
    MetaGetValue {
        /// MBTiles file to read a value from
        file: PathBuf,
        /// Value to read
        key: String,
    },
    // /// Sets a single value in the metadata table, or deletes it if no value.
    // #[command(name = "meta-set")]
    // MetaSetValue {
    //     /// MBTiles file to modify
    //     file: PathBuf,
    // },
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
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    match args.command {
        Commands::MetaGetValue { file, key } => {
            meta_get_value(file.as_path(), &key).await?;
        }
        Commands::Copy(opts) => {
            copy_mbtiles_file(opts).await?;
        }
        Commands::ApplyDiff {
            src_file,
            diff_file,
        } => {
            apply_mbtiles_diff(src_file, diff_file).await?;
        }
    }

    Ok(())
}

async fn meta_get_value(file: &Path, key: &str) -> Result<()> {
    let mbt = Mbtiles::new(file)?;
    let opt = SqliteConnectOptions::new().filename(file).read_only(true);
    let mut conn = SqliteConnection::connect_with(&opt).await?;
    if let Some(s) = mbt.get_metadata_value(&mut conn, key).await? {
        println!("{s}")
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use clap::error::ErrorKind;
    use clap::Parser;
    use martin_mbtiles::TileCopierOptions;

    use crate::Args;
    use crate::Commands::{ApplyDiff, Copy, MetaGetValue};

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
    fn test_copy_diff_with_file_no_force_simple_arguments() {
        assert_eq!(
            Args::try_parse_from([
                "mbtiles",
                "copy",
                "src_file",
                "dst_file",
                "--diff-with-file",
                "no_file",
            ])
            .unwrap_err()
            .kind(),
            ErrorKind::MissingRequiredArgument
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
                "--force-simple"
            ]),
            Args {
                verbose: false,
                command: Copy(
                    TileCopierOptions::new(PathBuf::from("src_file"), PathBuf::from("dst_file"))
                        .diff_with_file(PathBuf::from("no_file"))
                        .force_simple(true)
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
}
