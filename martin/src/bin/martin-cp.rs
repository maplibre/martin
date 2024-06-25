use std::borrow::Cow;
use std::fmt::{Debug, Display, Formatter};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use actix_http::error::ParseError;
use actix_http::test::TestRequest;
use actix_web::http::header::{AcceptEncoding, Header as _, ACCEPT_ENCODING};
use clap::Parser;
use futures::stream::{self, StreamExt};
use futures::TryStreamExt;
use log::{debug, error, info, log_enabled};
use martin::args::{Args, ExtraArgs, MetaArgs, OsEnv, SrvArgs};
use martin::srv::{merge_tilejson, DynTileSource};
use martin::{
    append_rect, read_config, Config, MartinError, MartinResult, ServerState, Source, TileData,
    TileRect,
};
use martin_tile_utils::{bbox_to_xyz, TileCoord, TileInfo};
use mbtiles::sqlx::SqliteConnection;
use mbtiles::UpdateZoomType::GrowOnly;
use mbtiles::{
    init_mbtiles_schema, is_empty_database, CopyDuplicateMode, MbtError, MbtType, MbtTypeCli,
    Mbtiles,
};
use tilejson::Bounds;
use tokio::sync::mpsc::channel;
use tokio::time::Instant;
use tokio::try_join;

const VERSION: &str = env!("CARGO_PKG_VERSION");
const SAVE_EVERY: Duration = Duration::from_secs(60);
const PROGRESS_REPORT_AFTER: u64 = 100;
const PROGRESS_REPORT_EVERY: Duration = Duration::from_secs(2);
const BATCH_SIZE: usize = 1000;

#[derive(Parser, Debug, PartialEq, Default)]
#[command(
    about = "A tool to bulk copy tiles from any Martin-supported sources into an mbtiles file",
    version,
    after_help = "Use RUST_LOG environment variable to control logging level, e.g. RUST_LOG=debug or RUST_LOG=martin_cp=debug. See https://docs.rs/env_logger/latest/env_logger/index.html#enabling-logging for more information."
)]
pub struct CopierArgs {
    #[command(flatten)]
    pub copy: CopyArgs,
    #[command(flatten)]
    pub meta: MetaArgs,
    #[cfg(feature = "postgres")]
    #[command(flatten)]
    pub pg: Option<martin::args::PgArgs>,
}

#[serde_with::serde_as]
#[derive(clap::Args, Debug, PartialEq, Default, serde::Deserialize, serde::Serialize)]
pub struct CopyArgs {
    /// Name of the source to copy from.
    #[arg(short, long)]
    pub source: String,
    /// Path to the mbtiles file to copy to.
    #[arg(short, long)]
    pub output_file: PathBuf,
    /// Output format of the new destination file. Ignored if the file exists. Defaults to 'normalized'.
    #[arg(
        long = "mbtiles-type",
        alias = "dst-type",
        value_name = "SCHEMA",
        value_enum
    )]
    pub mbt_type: Option<MbtTypeCli>,
    /// Optional query parameter (in URL query format) for the sources that support it (e.g. Postgres functions)
    #[arg(long)]
    pub url_query: Option<String>,
    /// Optional accepted encoding parameter as if the browser sent it in the HTTP request.
    /// If set to multiple values like `gzip,br`, martin-cp will use the first encoding,
    /// or re-encode if the tile is already encoded and that encoding is not listed.
    /// Use `identity` to disable compression. Ignored for non-encodable tiles like PNG and JPEG.
    #[arg(long, alias = "encodings", default_value = "gzip")]
    pub encoding: String,
    /// Allow copying to existing files, and indicate what to do if a tile with the same Z/X/Y already exists
    #[arg(long, value_enum)]
    pub on_duplicate: Option<CopyDuplicateMode>,
    /// Number of concurrent connections to use.
    #[arg(long, default_value = "1")]
    pub concurrency: Option<usize>,
    /// Bounds to copy, in the format `min_lon,min_lat,max_lon,max_lat`. Can be specified multiple times. Overlapping regions will be handled correctly.
    #[arg(long)]
    pub bbox: Vec<Bounds>,
    /// Minimum zoom level to copy
    #[arg(long, alias = "minzoom", conflicts_with("zoom_levels"))]
    pub min_zoom: Option<u8>,
    /// Maximum zoom level to copy
    #[arg(
        long,
        alias = "maxzoom",
        conflicts_with("zoom_levels"),
        required_unless_present("zoom_levels")
    )]
    pub max_zoom: Option<u8>,
    /// List of zoom levels to copy
    #[arg(short, long, alias = "zooms", value_delimiter = ',')]
    pub zoom_levels: Vec<u8>,
    /// Skip generating a global hash for mbtiles validation. By default, `martin-cp` will compute and update `agg_tiles_hash` metadata value.
    #[arg(long)]
    pub skip_agg_tiles_hash: bool,
    /// Set additional metadata values. Must be set as "key=value" pairs. Can be specified multiple times.
    #[arg(long, value_name="KEY=VALUE", value_parser = parse_key_value)]
    pub set_meta: Vec<(String, String)>,
}

fn parse_key_value(s: &str) -> Result<(String, String), String> {
    let mut parts = s.splitn(2, '=');
    let key = parts.next().unwrap();
    let value = parts
        .next()
        .ok_or_else(|| format!("Invalid key=value pair: {s}"))?;
    if key.is_empty() || value.is_empty() {
        Err(format!("Invalid key=value pair: {s}"))
    } else {
        Ok((key.to_string(), value.to_string()))
    }
}

async fn start(copy_args: CopierArgs) -> MartinCpResult<()> {
    info!("martin-cp tile copier v{VERSION}");

    let env = OsEnv::default();
    let save_config = copy_args.meta.save_config.clone();
    let mut config = if let Some(ref cfg_filename) = copy_args.meta.config {
        info!("Using {}", cfg_filename.display());
        read_config(cfg_filename, &env)?
    } else {
        info!("Config file is not specified, auto-detecting sources");
        Config::default()
    };

    let args = Args {
        meta: copy_args.meta,
        extras: ExtraArgs::default(),
        srv: SrvArgs::default(),
        #[cfg(feature = "postgres")]
        pg: copy_args.pg,
    };

    args.merge_into_config(&mut config, &env)?;
    config.finalize()?;

    let sources = config.resolve().await?;

    if let Some(file_name) = save_config {
        config.save_to_file(file_name)?;
    } else {
        info!("Use --save-config to save or print configuration.");
    }

    run_tile_copy(copy_args.copy, sources).await
}

fn compute_tile_ranges(args: &CopyArgs) -> Vec<TileRect> {
    let mut ranges = Vec::new();
    let boxes = if args.bbox.is_empty() {
        vec![Bounds::MAX_TILED]
    } else {
        args.bbox.clone()
    };
    for zoom in get_zooms(args).iter() {
        for bbox in &boxes {
            let (min_x, min_y, max_x, max_y) =
                bbox_to_xyz(bbox.left, bbox.bottom, bbox.right, bbox.top, *zoom);
            append_rect(
                &mut ranges,
                TileRect::new(*zoom, min_x, min_y, max_x, max_y),
            );
        }
    }
    ranges
}

fn get_zooms(args: &CopyArgs) -> Cow<Vec<u8>> {
    if let Some(max_zoom) = args.max_zoom {
        let mut zooms_vec = Vec::new();
        let min_zoom = args.min_zoom.unwrap_or(0);
        zooms_vec.extend(min_zoom..=max_zoom);
        Cow::Owned(zooms_vec)
    } else {
        Cow::Borrowed(&args.zoom_levels)
    }
}

struct TileXyz {
    xyz: TileCoord,
    data: TileData,
}

impl Debug for TileXyz {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} - {} bytes", self.xyz, self.data.len())
    }
}

struct Progress {
    // needed to compute elapsed time
    start_time: Instant,
    total: u64,
    empty: AtomicU64,
    non_empty: AtomicU64,
}

impl Progress {
    pub fn new(tiles: &[TileRect]) -> Self {
        let total = tiles.iter().map(TileRect::size).sum();
        Progress {
            start_time: Instant::now(),
            total,
            empty: AtomicU64::default(),
            non_empty: AtomicU64::default(),
        }
    }
}

type MartinCpResult<T> = Result<T, MartinCpError>;

#[derive(Debug, thiserror::Error)]
enum MartinCpError {
    #[error(transparent)]
    Martin(#[from] MartinError),
    #[error("Unable to parse encodings argument: {0}")]
    EncodingParse(#[from] ParseError),
    #[error(transparent)]
    Actix(#[from] actix_web::Error),
    #[error(transparent)]
    Mbt(#[from] MbtError),
}

impl Display for Progress {
    #[allow(clippy::cast_precision_loss)]
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let elapsed = self.start_time.elapsed();
        let elapsed_s = elapsed.as_secs_f32();
        let non_empty = self.non_empty.load(Ordering::Relaxed);
        let empty = self.empty.load(Ordering::Relaxed);
        let done = non_empty + empty;
        let percent = done * 100 / self.total;
        let speed = if elapsed_s > 0.0 {
            done as f32 / elapsed_s
        } else {
            0.0
        };
        write!(
            f,
            "[{elapsed:.1?}] {percent:.2}% @ {speed:.1}/s | ✓ {non_empty} □ {empty}"
        )?;

        let left = self.total - done;
        if left == 0 {
            f.write_str(" | done")
        } else if done == 0 {
            f.write_str(" | ??? left")
        } else {
            let left = Duration::from_secs_f32(elapsed_s * left as f32 / done as f32);
            write!(f, " | {left:.0?} left")
        }
    }
}

/// Given a list of tile ranges, iterate over all tiles in the ranges
fn iterate_tiles(tiles: Vec<TileRect>) -> impl Iterator<Item = TileCoord> {
    tiles.into_iter().flat_map(|t| {
        let z = t.zoom;
        (t.min_x..=t.max_x)
            .flat_map(move |x| (t.min_y..=t.max_y).map(move |y| TileCoord { z, x, y }))
    })
}

async fn run_tile_copy(args: CopyArgs, state: ServerState) -> MartinCpResult<()> {
    let output_file = &args.output_file;
    let concurrency = args.concurrency.unwrap_or(1);

    let src = DynTileSource::new(
        &state.tiles,
        args.source.as_str(),
        None,
        args.url_query.as_deref().unwrap_or_default(),
        Some(parse_encoding(args.encoding.as_str())?),
        None,
        None,
    )?;
    // parallel async below uses move, so we must only use copyable types
    let src = &src;

    let (tx, mut rx) = channel::<TileXyz>(500);
    let tiles = compute_tile_ranges(&args);
    let mbt = Mbtiles::new(output_file)?;
    let mut conn = mbt.open_or_new().await?;
    let on_duplicate = if let Some(on_duplicate) = args.on_duplicate {
        on_duplicate
    } else if !is_empty_database(&mut conn).await? {
        return Err(MbtError::DestinationFileExists(output_file.clone()).into());
    } else {
        CopyDuplicateMode::Override
    };
    let mbt_type = init_schema(&mbt, &mut conn, src.sources.as_slice(), src.info, &args).await?;

    let progress = Progress::new(&tiles);
    info!(
        "Copying {} {} tiles from {} to {}",
        progress.total,
        src.info,
        args.source,
        args.output_file.display()
    );

    try_join!(
        // Note: for some reason, tests hang here without the `move` keyword
        async move {
            stream::iter(iterate_tiles(tiles))
                .map(MartinResult::Ok)
                .try_for_each_concurrent(concurrency, |xyz| {
                    let tx = tx.clone();
                    async move {
                        let tile = src.get_tile_content(xyz).await?;
                        let data = tile.data;
                        tx.send(TileXyz { xyz, data })
                            .await
                            .map_err(|e| MartinError::InternalError(e.into()))?;
                        Ok(())
                    }
                })
                .await
        },
        async {
            let mut last_saved = Instant::now();
            let mut last_reported = Instant::now();
            let mut batch = Vec::with_capacity(BATCH_SIZE);
            while let Some(tile) = rx.recv().await {
                debug!("Generated tile {tile:?}");
                let done = if tile.data.is_empty() {
                    progress.empty.fetch_add(1, Ordering::Relaxed)
                } else {
                    batch.push((tile.xyz.z, tile.xyz.x, tile.xyz.y, tile.data));
                    if batch.len() >= BATCH_SIZE || last_saved.elapsed() > SAVE_EVERY {
                        mbt.insert_tiles(&mut conn, mbt_type, on_duplicate, &batch)
                            .await?;
                        batch.clear();
                        last_saved = Instant::now();
                    }
                    progress.non_empty.fetch_add(1, Ordering::Relaxed)
                };
                if done % PROGRESS_REPORT_AFTER == (PROGRESS_REPORT_AFTER - 1)
                    && last_reported.elapsed() > PROGRESS_REPORT_EVERY
                {
                    info!("{progress}");
                    last_reported = Instant::now();
                }
            }
            if !batch.is_empty() {
                mbt.insert_tiles(&mut conn, mbt_type, on_duplicate, &batch)
                    .await?;
            }
            Ok(())
        }
    )?;

    info!("{progress}");

    mbt.update_metadata(&mut conn, GrowOnly).await?;

    for (key, value) in args.set_meta {
        info!("Setting metadata key={key} value={value}");
        mbt.set_metadata_value(&mut conn, &key, value).await?;
    }

    if !args.skip_agg_tiles_hash {
        if progress.non_empty.load(Ordering::Relaxed) == 0 {
            info!("No tiles were copied, skipping agg_tiles_hash computation");
        } else {
            info!("Computing agg_tiles_hash value...");
            mbt.update_agg_tiles_hash(&mut conn).await?;
        }
    }

    Ok(())
}

fn parse_encoding(encoding: &str) -> MartinCpResult<AcceptEncoding> {
    let req = TestRequest::default()
        .insert_header((ACCEPT_ENCODING, encoding))
        .finish();
    Ok(AcceptEncoding::parse(&req)?)
}

async fn init_schema(
    mbt: &Mbtiles,
    conn: &mut SqliteConnection,
    sources: &[&dyn Source],
    tile_info: TileInfo,
    args: &CopyArgs,
) -> Result<MbtType, MartinError> {
    Ok(if is_empty_database(&mut *conn).await? {
        let mbt_type = match args.mbt_type.unwrap_or(MbtTypeCli::Normalized) {
            MbtTypeCli::Flat => MbtType::Flat,
            MbtTypeCli::FlatWithHash => MbtType::FlatWithHash,
            MbtTypeCli::Normalized => MbtType::Normalized { hash_view: true },
        };
        init_mbtiles_schema(&mut *conn, mbt_type).await?;
        let mut tj = merge_tilejson(sources, String::new());
        tj.other.insert(
            "format".to_string(),
            serde_json::Value::String(tile_info.format.metadata_format_value().to_string()),
        );
        tj.other.insert(
            "generator".to_string(),
            serde_json::Value::String(format!("martin-cp v{VERSION}")),
        );
        let zooms = get_zooms(args);
        if let Some(min_zoom) = zooms.iter().min() {
            tj.minzoom = Some(*min_zoom);
        }
        if let Some(max_zoom) = zooms.iter().max() {
            tj.maxzoom = Some(*max_zoom);
        }
        mbt.insert_metadata(&mut *conn, &tj).await?;
        mbt_type
    } else {
        mbt.detect_type(&mut *conn).await?
    })
}

#[actix_web::main]
async fn main() {
    let env = env_logger::Env::default().default_filter_or("martin_cp=info");
    env_logger::Builder::from_env(env).init();

    if let Err(e) = start(CopierArgs::parse()).await {
        // Ensure the message is printed, even if the logging is disabled
        if log_enabled!(log::Level::Error) {
            error!("{e}");
        } else {
            eprintln!("{e}");
        }
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use insta::assert_yaml_snapshot;

    use super::*;

    #[test]
    fn test_compute_tile_ranges() {
        let world = Bounds::MAX_TILED;
        let bbox_ca = Bounds::from_str("-124.482,32.5288,-114.1307,42.0095").unwrap();
        let bbox_ca_south = Bounds::from_str("-118.6681,32.5288,-114.1307,34.8233").unwrap();
        let bbox_mi = Bounds::from_str("-86.6271,41.6811,-82.3095,45.8058").unwrap();
        let bbox_usa = Bounds::from_str("-124.8489,24.3963,-66.8854,49.3843").unwrap();

        assert_yaml_snapshot!(compute_tile_ranges(&args(&[world], &[0])), @r###"
        ---
        - "0: (0,0) - (0,0)"
        "###);

        assert_yaml_snapshot!(compute_tile_ranges(&args(&[world], &[3,7])), @r###"
        ---
        - "3: (0,0) - (7,7)"
        - "7: (0,0) - (127,127)"
        "###);

        assert_yaml_snapshot!(compute_tile_ranges(&arg_minmax(&[world], 2, 4)), @r###"
        ---
        - "2: (0,0) - (3,3)"
        - "3: (0,0) - (7,7)"
        - "4: (0,0) - (15,15)"
        "###);

        assert_yaml_snapshot!(compute_tile_ranges(&args(&[world], &[14])), @r###"
        ---
        - "14: (0,0) - (16383,16383)"
        "###);

        assert_yaml_snapshot!(compute_tile_ranges(&args(&[bbox_usa], &[14])), @r###"
        ---
        - "14: (2509,5599) - (5147,7046)"
        "###);

        assert_yaml_snapshot!(compute_tile_ranges(&args(&[bbox_usa, bbox_mi, bbox_ca], &[14])), @r###"
        ---
        - "14: (2509,5599) - (5147,7046)"
        "###);

        assert_yaml_snapshot!(compute_tile_ranges(&args(&[bbox_ca_south, bbox_mi, bbox_ca], &[14])), @r###"
        ---
        - "14: (2791,6499) - (2997,6624)"
        - "14: (4249,5841) - (4446,6101)"
        - "14: (2526,6081) - (2790,6624)"
        - "14: (2791,6081) - (2997,6498)"
        "###);
    }

    fn args(bbox: &[Bounds], zooms: &[u8]) -> CopyArgs {
        CopyArgs {
            bbox: bbox.to_vec(),
            zoom_levels: zooms.to_vec(),
            ..Default::default()
        }
    }

    fn arg_minmax(bbox: &[Bounds], min_zoom: u8, max_zoom: u8) -> CopyArgs {
        CopyArgs {
            bbox: bbox.to_vec(),
            min_zoom: Some(min_zoom),
            max_zoom: Some(max_zoom),
            ..Default::default()
        }
    }
}
