use std::borrow::Cow;
use std::fmt::{Debug, Display, Formatter};
use std::num::NonZeroUsize;
use std::ops::RangeInclusive;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use actix_http::error::ParseError;
use actix_http::test::TestRequest;
use actix_web::http::header::{ACCEPT_ENCODING, AcceptEncoding, Header as _};
use clap::Parser;
use clap::builder::Styles;
use clap::builder::styling::AnsiColor;
use futures::TryStreamExt;
use futures::stream::{self, StreamExt};
use martin::config::args::{Args, ExtraArgs, MetaArgs, SrvArgs};
use martin::config::file::{Config, ServerState, read_config};
use martin::logging::{ensure_martin_core_log_level_matches, init_tracing};
use martin::srv::{DynTileSource, merge_tilejson};
use martin::{MartinError, MartinResult};
use martin_core::config::env::OsEnv;
use martin_core::tiles::BoxedSource;
use martin_core::tiles::mbtiles::MbtilesError;
use martin_tile_utils::{TileCoord, TileData, TileInfo, TileRect, append_rect, bbox_to_xyz};
use mbtiles::UpdateZoomType::GrowOnly;
use mbtiles::sqlx::SqliteConnection;
use mbtiles::{
    CopyDuplicateMode, MbtError, MbtType, MbtTypeCli, Mbtiles, init_mbtiles_schema,
    is_empty_database,
};
use tilejson::Bounds;
use tokio::sync::mpsc::channel;
use tokio::time::Instant;
use tokio::try_join;
use tracing::{debug, error, info, warn};

const VERSION: &str = env!("CARGO_PKG_VERSION");
const SAVE_EVERY: Duration = Duration::from_secs(60);
const PROGRESS_REPORT_AFTER: u64 = 100;
const PROGRESS_REPORT_EVERY: Duration = Duration::from_secs(2);
const BATCH_SIZE: usize = 1000;
/// Defines the styles used for the CLI help output.
const HELP_STYLES: Styles = Styles::styled()
    .header(AnsiColor::Blue.on_default().bold())
    .usage(AnsiColor::Blue.on_default().bold())
    .literal(AnsiColor::White.on_default())
    .placeholder(AnsiColor::Green.on_default());

#[derive(Parser, Debug, PartialEq)]
#[command(
    about = "A tool to bulk copy tiles from any Martin-supported sources into an mbtiles file",
    version,
    after_help = "Use RUST_LOG environment variable to control logging level, e.g. RUST_LOG=debug or RUST_LOG=martin_cp=debug.\nUse MARTIN_CP_FORMAT environment variable to control output format: compact (default), full, pretty, or json.\nSee https://docs.rs/tracing-subscriber/latest/tracing_subscriber/filter/struct.EnvFilter.html for more information.",
    styles = HELP_STYLES
)]
pub struct CopierArgs {
    #[command(flatten)]
    pub copy: CopyArgs,
    #[command(flatten)]
    pub meta: MetaArgs,
    #[cfg(feature = "postgres")]
    #[command(flatten)]
    pub pg: Option<martin::config::args::PostgresArgs>,
}

#[serde_with::serde_as]
#[derive(clap::Args, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct CopyArgs {
    /// Name of the source to copy from. Not required if there is only one source.
    #[arg(short, long)]
    pub source: Option<String>,
    /// Path to the mbtiles file to copy to.
    #[arg(short, long)]
    pub output_file: PathBuf,
    /// Output format of the new destination file. Ignored if the file exists. [DEFAULT: normalized]
    #[arg(
        long = "mbtiles-type",
        alias = "dst-type",
        value_name = "SCHEMA",
        value_enum
    )]
    pub mbt_type: Option<MbtTypeCli>,
    /// Optional query parameter (in URL query format) for the sources that support it (e.g. Postgres functions).
    #[arg(long)]
    pub url_query: Option<String>,
    /// Optional accepted encoding parameter as if the browser sent it in the HTTP request.
    ///
    /// If set to multiple values like `gzip,br`, martin-cp will use the first encoding,
    /// or re-encode if the tile is already encoded and that encoding is not listed.
    /// Use `identity` to disable compression. Ignored for non-encodable tiles like PNG and JPEG.
    #[arg(long, alias = "encodings", default_value = "gzip")]
    pub encoding: String,
    /// Allow copying to existing files, and indicate what to do if a tile with the same Z/X/Y already exists.
    #[arg(long, value_enum)]
    pub on_duplicate: Option<CopyDuplicateMode>,
    /// Number of concurrent connections to use.
    #[arg(long, default_value = "1")]
    pub concurrency: NonZeroUsize,
    /// Bounds to copy, in the format `min_lon,min_lat,max_lon,max_lat`. Can be specified multiple times with overlapping bounds being handled correctly. Maximum bounds follows mbtiles specification for xyz-compliant tile bounds.
    ///
    /// If omitted, will first default to configured source bounds if present. Otherwise, will default to global xyz-compliant tile bounds.
    #[arg(long, default_value = "-180,-85.05112877980659,180,85.0511287798066")]
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
    /// Set additional metadata values. Must be set as `"key=value"` pairs. Can be specified multiple times.
    #[arg(long, value_name="KEY=VALUE", value_parser = parse_key_value)]
    pub set_meta: Vec<(String, String)>,
}

impl Default for CopyArgs {
    fn default() -> Self {
        CopyArgs {
            bbox: Vec::new(),
            source: None,
            output_file: PathBuf::new(),
            mbt_type: None,
            url_query: None,
            encoding: "gzip".to_string(),
            on_duplicate: None,
            concurrency: NonZeroUsize::new(1).expect("1 is larger than 0"),
            min_zoom: None,
            max_zoom: None,
            zoom_levels: Vec::new(),
            skip_agg_tiles_hash: true,
            set_meta: Vec::new(),
        }
    }
}

fn parse_key_value(s: &str) -> Result<(String, String), String> {
    let mut parts = s.splitn(2, '=');
    let key = parts
        .next()
        .ok_or_else(|| format!("Invalid key=value pair: {s}"))?;
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
        read_config(cfg_filename, &env).map_err(MartinError::from)?
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
        config
            .save_to_file(file_name.as_path())
            .map_err(MartinError::from)?;
    } else {
        info!("Use --save-config to save or print configuration.");
    }

    run_tile_copy(copy_args.copy, sources).await
}

fn check_bboxes(boxes: Vec<Bounds>) -> MartinCpResult<Vec<Bounds>> {
    for bb in &boxes {
        let allowed_lon = Bounds::MAX_TILED.left..=Bounds::MAX_TILED.right;
        if !allowed_lon.contains(&bb.left) || !allowed_lon.contains(&bb.right) {
            return Err(MartinCpError::InvalidBoundingBox(
                "longitude",
                *bb,
                allowed_lon,
            ));
        }
        let allowed_lat = Bounds::MAX_TILED.bottom..=Bounds::MAX_TILED.top;
        if !allowed_lat.contains(&bb.bottom) || !allowed_lat.contains(&bb.top) {
            return Err(MartinCpError::InvalidBoundingBox(
                "latitude",
                *bb,
                allowed_lat,
            ));
        }
    }
    Ok(boxes)
}

fn compute_tile_ranges(boxes: &[Bounds], zooms: &[u8]) -> Vec<TileRect> {
    let mut ranges = Vec::new();
    for zoom in zooms {
        for bbox in boxes {
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

fn get_zooms(args: &CopyArgs) -> Cow<'_, [u8]> {
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

#[derive(thiserror::Error, Debug)]
enum MartinCpError {
    #[error(transparent)]
    Martin(#[from] MartinError),
    #[error("Unable to parse encodings argument: {0}")]
    EncodingParse(#[from] ParseError),
    #[error(transparent)]
    Actix(#[from] actix_web::Error),
    #[error(transparent)]
    Mbt(#[from] MbtError),
    #[error("No sources found")]
    NoSources,
    #[error(
        "More than one source found, please specify source using --source.\nAvailable sources: {0}"
    )]
    MultipleSources(String),
    #[error(
        "{0} of bounding box '{1}' must fit into {2:?}. Please check that your bounding box is in the `min_lon,min_lat,max_lon,max_lat` format."
    )]
    InvalidBoundingBox(&'static str, Bounds, RangeInclusive<f64>),
}

impl Display for Progress {
    #[expect(clippy::cast_precision_loss)]
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

fn check_sources(args: &CopyArgs, state: &ServerState) -> Result<String, MartinCpError> {
    if let Some(source_id) = &args.source {
        Ok(source_id.clone())
    } else {
        let source_ids = state.tiles.source_names();
        if let Some(source_id) = source_ids.first() {
            if source_ids.len() > 1 {
                return Err(MartinCpError::MultipleSources(source_ids.join(", ")));
            }
            Ok(source_id.clone())
        } else {
            Err(MartinCpError::NoSources)
        }
    }
}

fn default_bounds(src: &DynTileSource) -> Vec<Bounds> {
    if src.sources.is_empty() {
        vec![Bounds::MAX_TILED]
    } else {
        let mut source_bounds = src
            .sources
            .iter()
            .map(|source| source.get_tilejson().bounds.unwrap_or(Bounds::MAX_TILED))
            .collect::<Vec<Bounds>>();

        source_bounds.dedup_by_key(|bounds| bounds.to_string());

        if source_bounds.is_empty() {
            info!(
                "No configured bounds for source, using: {}",
                Bounds::MAX_TILED
            );
            vec![Bounds::MAX_TILED]
        } else {
            info!(
                "No bbox specified, using source bounds: {}",
                source_bounds
                    .iter()
                    .map(|s| format!("[{s}]"))
                    .collect::<Vec<String>>()
                    .join(", ")
            );
            source_bounds
        }
    }
}

#[expect(clippy::too_many_lines)]
async fn run_tile_copy(args: CopyArgs, state: ServerState) -> MartinCpResult<()> {
    let output_file = &args.output_file;
    let concurrency = args.concurrency.get();
    // we only warn that the concurrency might be too low if:
    // - a user has concurrency at the default
    // - there is at least one pg or remote pmtiles source
    if concurrency == 1 && state.tiles.benefits_from_concurrent_scraping() {
        warn!(
            "Using `--concurrency 1`. Increasing it may improve performance for your tile sources. See https://docs.martin.rs/cli/usage.html#concurrency for further details."
        );
    }

    let source_id = check_sources(&args, &state)?;

    let src = DynTileSource::new(
        &state.tiles,
        &source_id,
        None,
        args.url_query.as_deref().unwrap_or_default(),
        Some(parse_encoding(args.encoding.as_str())?),
        None,
        None,
        None,
    )?;

    let inferred_bboxes = if args.bbox.is_empty() {
        default_bounds(&src)
    } else {
        args.bbox.clone()
    };
    let bboxes = check_bboxes(inferred_bboxes)?;

    // parallel async below uses move, so we must only use copyable types
    let src = &src;

    let zooms = get_zooms(&args);
    let tiles = compute_tile_ranges(&bboxes, &zooms);
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
        source_id,
        args.output_file.display()
    );

    let (tx, mut rx) = channel::<TileXyz>(500);
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
                            .expect("The receive half of the channel is not closed");
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
                            .await
                            .map_err(MbtilesError::from)?;
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
                    .await
                    .map_err(MbtilesError::from)?;
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
    sources: &[BoxedSource],
    tile_info: TileInfo,
    args: &CopyArgs,
) -> Result<MbtType, MartinError> {
    Ok(
        if is_empty_database(&mut *conn)
            .await
            .map_err(MbtilesError::from)?
        {
            let mbt_type = match args.mbt_type.unwrap_or(MbtTypeCli::Normalized) {
                MbtTypeCli::Flat => MbtType::Flat,
                MbtTypeCli::FlatWithHash => MbtType::FlatWithHash,
                MbtTypeCli::Normalized => MbtType::Normalized { hash_view: true },
            };
            init_mbtiles_schema(&mut *conn, mbt_type)
                .await
                .map_err(MbtilesError::from)?;
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
            mbt.insert_metadata(&mut *conn, &tj)
                .await
                .map_err(MbtilesError::from)?;
            mbt_type
        } else {
            mbt.detect_type(&mut *conn)
                .await
                .map_err(MbtilesError::from)?
        },
    )
}

#[actix_web::main]
async fn main() {
    let filter = ensure_martin_core_log_level_matches(std::env::var("RUST_LOG").ok(), "martin_cp=");
    init_tracing(&filter, std::env::var("MARTIN_CP_FORMAT").ok());

    let args = CopierArgs::parse();
    if let Err(e) = start(args).await {
        error!("{e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use async_trait::async_trait;
    use insta::assert_yaml_snapshot;
    use martin::TileSources;
    use martin_core::tiles::{MartinCoreResult, Source, UrlQuery};
    use martin_tile_utils::{Encoding, Format};
    use rstest::{fixture, rstest};
    use tilejson::{TileJSON, tilejson};

    use super::*;

    #[derive(Debug, Clone)]
    pub struct MockSource {
        pub id: &'static str,
        pub tj: TileJSON,
        pub data: TileData,
    }

    #[async_trait]
    impl Source for MockSource {
        fn get_id(&self) -> &str {
            self.id
        }

        fn get_tilejson(&self) -> &TileJSON {
            &self.tj
        }

        fn get_tile_info(&self) -> TileInfo {
            TileInfo::new(Format::Mvt, Encoding::Uncompressed)
        }

        fn clone_source(&self) -> BoxedSource {
            Box::new(self.clone())
        }

        async fn get_tile(
            &self,
            _xyz: TileCoord,
            _url_query: Option<&UrlQuery>,
        ) -> MartinCoreResult<TileData> {
            Ok(self.data.clone())
        }
    }

    #[fixture]
    fn many_sources() -> TileSources {
        TileSources::new(vec![vec![
            Box::new(MockSource {
                id: "test_source",
                tj: tilejson! { tiles: vec![], bounds: Bounds::from_str("-110.0,20.0,-120.0,80.0").unwrap() },
                data: Vec::default(),
            }),
            Box::new(MockSource {
                id: "test_source2",
                tj: tilejson! { tiles: vec![], bounds: Bounds::from_str("-130.0,40.0,-170.0,10.0").unwrap() },
                data: Vec::default(),
            }),
            Box::new(MockSource {
                id: "unrequested_source",
                tj: tilejson! { tiles: vec![], bounds: Bounds::from_str("-150.0,40.0,-120.0,10.0").unwrap() },
                data: Vec::default(),
            }),
            Box::new(MockSource {
                id: "unbounded_source",
                tj: tilejson! { tiles: vec![] },
                data: Vec::default(),
            }),
        ]])
    }

    #[fixture]
    fn one_source() -> TileSources {
        TileSources::new(vec![vec![Box::new(MockSource {
            id: "test_source",
            tj: tilejson! { tiles: vec![], bounds: Bounds::from_str("-120.0,30.0,-110.0,40.0").unwrap() },
            data: Vec::default(),
        })]])
    }

    #[fixture]
    fn source_wo_bounds() -> TileSources {
        TileSources::new(vec![vec![Box::new(MockSource {
            id: "test_source",
            tj: tilejson! { tiles: vec![] },
            data: Vec::default(),
        })]])
    }

    #[rstest]
    #[case::one_source(one_source(), "test_source", vec![Bounds::from_str("-120.0,30.0,-110.0,40.0").unwrap()])]
    #[case::many_sources(many_sources(), "test_source,test_source2", vec![Bounds::from_str("-110.0,20.0,-120.0,80.0").unwrap(), Bounds::from_str("-130.0,40.0,-170.0,10.0").unwrap()])]
    #[case::many_sources_rev(many_sources(), "test_source2,test_source", vec![Bounds::from_str("-130.0,40.0,-170.0,10.0").unwrap(), Bounds::from_str("-110.0,20.0,-120.0,80.0").unwrap()])]
    #[case::many_sources_only_unbounded(many_sources(), "unbounded_source", vec![Bounds::MAX_TILED])]
    #[case::many_sources_bounded_and_unbounded(many_sources(), "test_source,unbounded_source", vec![Bounds::from_str("-110.0,20.0,-120.0,80.0").unwrap(), Bounds::MAX_TILED])]
    #[case::many_sources_bounded_and_unbounded_rev(many_sources(), "unbounded_source,test_source", vec![Bounds::MAX_TILED, Bounds::from_str("-110.0,20.0,-120.0,80.0").unwrap()])]
    #[case::source_wo_bounds(source_wo_bounds(), "test_source", vec![Bounds::MAX_TILED])]
    fn test_default_bounds(
        #[case] src: TileSources,
        #[case] ids: &str,
        #[case] expected: Vec<Bounds>,
    ) {
        let dts = DynTileSource::new(&src, ids, None, "", None, None, None, None).unwrap();

        assert_eq!(default_bounds(&dts), expected);
    }

    #[test]
    fn test_compute_tile_ranges() {
        let world = Bounds::MAX_TILED;
        let bbox_ca = Bounds::from_str("-124.482,32.5288,-114.1307,42.0095").unwrap();
        let bbox_ca_south = Bounds::from_str("-118.6681,32.5288,-114.1307,34.8233").unwrap();
        let bbox_mi = Bounds::from_str("-86.6271,41.6811,-82.3095,45.8058").unwrap();
        let bbox_usa = Bounds::from_str("-124.8489,24.3963,-66.8854,49.3843").unwrap();

        assert_yaml_snapshot!(compute_tile_ranges(&[world], &[0]), @r#"- "0: (0,0) - (0,0)""#);

        assert_yaml_snapshot!(compute_tile_ranges(&[world], &[3,7]), @r#"
        - "3: (0,0) - (7,7)"
        - "7: (0,0) - (127,127)"
        "#);

        assert_yaml_snapshot!(compute_tile_ranges(&[world], &[2, 3, 4]), @r#"
        - "2: (0,0) - (3,3)"
        - "3: (0,0) - (7,7)"
        - "4: (0,0) - (15,15)"
        "#);

        assert_yaml_snapshot!(compute_tile_ranges(&[world], &[14]), @r#"- "14: (0,0) - (16383,16383)""#);

        assert_yaml_snapshot!(compute_tile_ranges(&[bbox_usa], &[14]), @r#"- "14: (2509,5599) - (5147,7046)""#);

        assert_yaml_snapshot!(compute_tile_ranges(&[bbox_usa, bbox_mi, bbox_ca], &[14]), @r#"- "14: (2509,5599) - (5147,7046)""#);

        assert_yaml_snapshot!(compute_tile_ranges(&[bbox_ca_south, bbox_mi, bbox_ca], &[14]), @r#"
        - "14: (2791,6499) - (2997,6624)"
        - "14: (4249,5841) - (4446,6101)"
        - "14: (2526,6081) - (2790,6624)"
        - "14: (2791,6081) - (2997,6498)"
        "#);
    }

    #[rstest]
    #[case("-180.0,-85.05112877980659,180.0,85.0511287798066", Ok(Bounds::MAX_TILED.to_string()))]
    #[case("-120.0,30.0,-110.0,40.0", Ok("-120.0,30.0,-110.0,40.0".to_string()))]
    #[case("-190.0,30.0,-110.0,40.0", Err("longitude".to_string()))]
    #[case("-120.0,30.0,190.0,40.0", Err("longitude".to_string()))]
    #[case("-120.0,-90.0,-110.0,40.0", Err("latitude".to_string()))]
    #[case("-120.0,30.0,-110.0,90.0", Err("latitude".to_string()))]
    fn test_check_bboxes(#[case] bbox_str: &str, #[case] expected: Result<String, String>) {
        use std::str::FromStr;

        let bbox_vec = if bbox_str.is_empty() {
            vec![]
        } else {
            vec![Bounds::from_str(bbox_str).unwrap()]
        };

        let result = check_bboxes(bbox_vec);

        match expected {
            Ok(expected_str) => {
                let expected_bound = Bounds::from_str(&expected_str).unwrap();
                assert_eq!(result.unwrap(), vec![expected_bound]);
            }
            Err(expected_coord) => {
                assert!(matches!(
                    result,
                    Err(MartinCpError::InvalidBoundingBox(coord, _, _)) if coord == expected_coord
                ));
            }
        }
    }

    #[rstest]
    #[case(None, None, vec![], vec![])] // !min && !max => levels
    #[case(None, None, vec![1, 3], vec![1, 3])] // !min && !max => levels
    #[case(None, Some(5), vec![], vec![])] // !min => levels
    #[case(None, Some(5), vec![3], vec![3])] // !min => levels
    #[case(Some(2), None, vec![], vec![0, 1, 2])] // max && !min => 0..=max
    #[case(Some(5), Some(2), vec![], vec![2, 3, 4, 5])] // min > max
    #[case(Some(2), Some(5), vec![], vec![])] // min < max
    #[case(Some(4), Some(4), vec![], vec![4])] // min = max
    fn test_get_zooms(
        #[case] max_zoom: Option<u8>,
        #[case] min_zoom: Option<u8>,
        #[case] zoom_levels: Vec<u8>,
        #[case] expected: Vec<u8>,
    ) {
        let args = CopyArgs {
            min_zoom,
            max_zoom,
            zoom_levels,
            ..Default::default()
        };

        assert_eq!(get_zooms(&args).as_ref(), expected.as_slice());
    }
}
