use anyhow::Result;
use clap::{Parser, Subcommand};
use martin_mbtiles::Mbtiles;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    version,
    name = "mbtiles",
    about = "A utility to work with .mbtiles files content"
)]
pub struct Args {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Prints all values in the metadata table.
    #[command(name = "meta-all")]
    MetaAll {
        /// MBTiles file to read from
        file: PathBuf,
    },
    /// Gets a single value from metadata table.
    #[command(name = "meta-get")]
    MetaGetValue {
        /// MBTiles file to read a value from
        file: PathBuf,
        /// Value to read
        key: String,
    },
    /// Sets a single value in the metadata table, or deletes it if no value.
    #[command(name = "meta-set")]
    MetaSetValue {
        /// MBTiles file to modify
        file: PathBuf,
    },
    /// Copy tiles from one mbtiles file to another.
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
            let mbt = Mbtiles::new(&file).await?;

            let value = mbt.get_metadata_value(&key).await?;

            match value {
                Some(s) => println!("The value for metadata key \"{key}\" is:\n \"{s}\""),
                None => println!("No value for metadata key \"{key}\""),
            }
        }
        _ => {
            unimplemented!("Oops! This command is not yet available, stay tuned for future updates")
        }
    }

    Ok(())
}
