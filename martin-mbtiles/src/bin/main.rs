use std::path::{Path, PathBuf};

use anyhow::Result;
use clap::{Parser, Subcommand};
use martin_mbtiles::{Mbtiles, TileCopier};
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{Connection, SqliteConnection};

#[derive(Parser, Debug)]
#[command(
    version,
    name = "mbtiles",
    about = "A utility to work with .mbtiles file content"
)]
pub struct Args {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
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
    Copy {
        /// MBTiles file to read from
        src_file: PathBuf,
        /// MBTiles file to write to
        dst_file: PathBuf,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    match args.command {
        Commands::MetaGetValue { file, key } => {
            meta_get_value(file.as_path(), &key).await?;
        }
        Commands::Copy { src_file, dst_file } => {
            copy_tiles(src_file.as_path(), dst_file.as_path()).await?;
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

async fn copy_tiles(src_file: &Path, dst_file: &Path) -> Result<()> {
    TileCopier::new(PathBuf::from(src_file), PathBuf::from(dst_file))
        .run()
        .await?;

    Ok(())
}
