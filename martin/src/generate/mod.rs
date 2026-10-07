//! `martin generate`: bulk tile generation from whole-table scans.

pub mod postgres;

use std::collections::{BTreeMap, HashSet};
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use martin_core::tiles::postgres::{PostgresError, PostgresPool};
use martin_tile_utils::{Encoding, WEB_MERCATOR_QUAD};
use martin_tilegen::{
    GenerateConfig, MbtilesSink, PmtilesSink, Progress, SortConfig, Summary, TileFormat,
    TileGenError, TileGenResult, generate,
};
use tilejson::{Bounds, tilejson};
use tracing::{info, warn};

use self::postgres::{PgScanSource, ScanLayer};
use crate::StartupError;
use crate::config::args::{
    Args, ArgsError, ExtraArgs, MetaArgs, PostgresArgs, SrvArgs, parse_byte_size,
};
use crate::config::file::postgres::{PostgresAutoDiscoveryBuilder, SourceSpec, TableInfo};
use crate::config::file::{Config, ConfigFileError, TileGrids, read_config};
use crate::config::primitives::IdResolver;
use crate::config::primitives::env::OsEnv;
use crate::cp::{MartinCpError, check_bboxes};
use crate::srv::RESERVED_KEYWORDS;

const VERSION: &str = env!("CARGO_PKG_VERSION");
const PROGRESS_EVERY: Duration = Duration::from_secs(10);
/// Per-worker sort buffers are kept between these: smaller spills too many runs, larger cannot be indexed.
const BUFFER_BYTES: std::ops::RangeInclusive<u64> = (16 << 20)..=(u32::MAX as u64);

#[derive(clap::Args, Debug, PartialEq)]
#[command(
    about = "Generate a tileset from whole tables in one pass (bulk generation)",
    after_help = "Each source is scanned once and rendered at every zoom; this is much faster than `martin cp` for whole tilesets."
)]
pub struct GeneratorArgs {
    #[command(flatten)]
    pub generate: GenerateArgs,
    #[command(flatten)]
    pub meta: MetaArgs,
    #[command(flatten)]
    pub pg: Option<PostgresArgs>,
}

#[derive(clap::Args, Debug, PartialEq)]
pub struct GenerateArgs {
    /// Table sources to render. Defaults to every table source.
    #[arg(short, long = "source", value_name = "ID")]
    pub sources: Vec<String>,
    /// The `.mbtiles` file to create, which must not exist or be empty, or a new `.pmtiles` archive.
    #[arg(short, long)]
    pub output_file: PathBuf,
    #[arg(long, value_enum, default_value = "mlt")]
    pub format: GenerateFormat,
    /// Tile compression: `gzip`, `zstd`, `br`, `zlib` (`MBTiles` only) or `none`.
    #[arg(long, default_value = "gzip", value_parser = parse_encoding)]
    pub encoding: Encoding,
    /// Lowest zoom to generate; a source's own `minzoom` raises it.
    #[arg(long, alias = "minzoom", default_value_t = 0)]
    pub min_zoom: u8,
    /// Highest zoom to generate; a source's own `maxzoom` lowers it.
    #[arg(long, alias = "maxzoom", default_value_t = 14)]
    pub max_zoom: u8,
    /// Only features intersecting `min_lon,min_lat,max_lon,max_lat` are rendered, and only tiles intersecting it
    /// are written. Unlike `martin cp`, low-zoom tiles are not filled with data from outside the box.
    #[arg(long)]
    pub bbox: Option<Bounds>,
    /// Memory for sorting, e.g. `512MB` or `2GiB`, split over the threads. A bare number is in MB.
    #[arg(long, default_value = "2GiB", value_parser = parse_byte_size)]
    pub memory: u64,
    /// Directories for temporary sorted runs, used round-robin. Defaults to the system temp directory.
    #[arg(long)]
    pub temp_dir: Vec<PathBuf>,
    /// Worker threads. Defaults to the number of CPUs.
    #[arg(long)]
    pub threads: Option<NonZeroUsize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum GenerateFormat {
    /// Mapbox Vector Tile.
    #[value(alias = "pbf")]
    Mvt,
    /// `MapLibre` Tile.
    Mlt,
}

#[derive(thiserror::Error, Debug)]
pub enum GenerateError {
    #[error(transparent)]
    Startup(#[from] StartupError),
    #[error(transparent)]
    Postgres(#[from] PostgresError),
    #[error(transparent)]
    TileGen(#[from] TileGenError),
    #[error(transparent)]
    Bbox(#[from] MartinCpError),
    #[error("No table source matches `{0}`. Available table sources: {1}")]
    UnknownSource(String, String),
    #[error("Source `{0}` is a function source; only tables and views can be scanned")]
    FunctionSource(String),
    #[error("Source `{0}` uses tile grid `{1}`; `martin generate` supports Web Mercator only")]
    UnsupportedGrid(String, String),
    #[error("Sources come from more than one database; generate them separately")]
    MultipleDatabases,
    #[error("No table sources to generate")]
    NoSources,
    #[error("Source `{0}` has no zoom between {1} and {2}")]
    NoZooms(String, u8, u8),
    #[error("Interrupted; removed the partial output {}", .0.display())]
    Interrupted(PathBuf),
    #[error("Background task failed: {0}")]
    Join(#[from] tokio::task::JoinError),
}

impl From<ConfigFileError> for GenerateError {
    fn from(err: ConfigFileError) -> Self {
        Self::Startup(err.into())
    }
}

impl From<ArgsError> for GenerateError {
    fn from(err: ArgsError) -> Self {
        Self::Startup(err.into())
    }
}

pub type GenerateResult<T> = Result<T, GenerateError>;

pub async fn start(args: GeneratorArgs) -> GenerateResult<()> {
    info!(martin.version = %VERSION, "Starting Martin tile generator");
    let started = Instant::now();
    let options = args.generate;
    if let Some(bbox) = options.bbox {
        check_bboxes(&WEB_MERCATOR_QUAD, vec![bbox])?;
    }
    let mut config = match &args.meta.config {
        Some(path) => read_config(path, &OsEnv)?,
        None => Config::default(),
    };
    Args {
        command: None,
        meta: args.meta,
        extras: ExtraArgs::default(),
        srv: SrvArgs::default(),
        pg: args.pg,
    }
    .merge_into_config(&mut config)?;
    config.finalize().await?;
    config.warn_unrecognized_keys();

    let (pool, layers) = select_sources(&config, &options).await?;
    let threads = options
        .threads
        .or_else(|| std::thread::available_parallelism().ok())
        .map_or(1, NonZeroUsize::get);
    let source = PgScanSource::new(pool, layers)?;
    let generate_config = generate_config(&options, threads);
    let metadata = metadata(&options);
    let output = options.output_file;
    let sink = {
        let (output, format, encoding) = (output.clone(), generate_config.format, options.encoding);
        tokio::task::spawn_blocking(move || Output::create(&output, format, encoding)).await??
    };

    let progress = Arc::new(Progress::default());
    let reporter = tokio::spawn(report(Arc::clone(&progress)));
    let task = tokio::task::spawn_blocking({
        let progress = Arc::clone(&progress);
        move || -> GenerateResult<Summary> {
            Ok(sink.generate(&source, &generate_config, metadata, &progress)?)
        }
    });
    let result = tokio::select! {
        result = task => result.map_err(GenerateError::from).and_then(|r| r),
        _ = tokio::signal::ctrl_c() => Err(GenerateError::Interrupted(output.clone())),
    };
    reporter.abort();
    let summary = result.inspect_err(|_| remove_output(&output))?;
    info!(
        features = summary.features,
        tiles = summary.tiles,
        elapsed = ?started.elapsed(),
        mbtiles.file = %output.display(),
        "Generated tiles"
    );
    if summary.skipped > 0 {
        warn!(
            features.skipped = summary.skipped,
            "Skipped features with geometry collections or empty geometries, which tiles cannot hold"
        );
    }
    for failed in &summary.slice_errors {
        warn!(
            layer.id = %failed.layer,
            feature_zooms.skipped = failed.errors,
            feature.id = failed.id,
            feature.partition = failed.partition,
            feature.row = failed.row,
            tile.zoom = failed.zoom,
            error = %failed.error,
            "Skipped feature-zooms whose geometry the slicer could not handle"
        );
    }
    Ok(())
}

/// Created before the run starts, so that a failure removes only a file this run created.
enum Output {
    Mbtiles(MbtilesSink),
    Pmtiles(PmtilesSink),
}

impl Output {
    fn create(path: &Path, format: TileFormat, encoding: Encoding) -> TileGenResult<Self> {
        if path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("pmtiles"))
        {
            PmtilesSink::create(path, format, encoding).map(Self::Pmtiles)
        } else {
            MbtilesSink::create(path).map(Self::Mbtiles)
        }
    }

    fn generate(
        self,
        source: &PgScanSource,
        config: &GenerateConfig,
        metadata: tilejson::TileJSON,
        progress: &Progress,
    ) -> TileGenResult<Summary> {
        match self {
            Self::Mbtiles(sink) => {
                generate(source, source.plan(), sink, config, metadata, progress)
            }
            Self::Pmtiles(sink) => {
                generate(source, source.plan(), sink, config, metadata, progress)
            }
        }
    }
}

fn generate_config(options: &GenerateArgs, threads: usize) -> GenerateConfig {
    let temp_dirs = if options.temp_dir.is_empty() {
        vec![std::env::temp_dir()]
    } else {
        options.temp_dir.clone()
    };
    let per_thread =
        (options.memory / threads as u64).clamp(*BUFFER_BYTES.start(), *BUFFER_BYTES.end());
    GenerateConfig {
        threads,
        sort: SortConfig {
            temp_dirs,
            buffer_bytes: usize::try_from(per_thread).unwrap_or(usize::MAX),
            max_fan_in: 256,
            read_buffer_bytes: 256 << 10,
        },
        format: match options.format {
            GenerateFormat::Mlt => TileFormat::Mlt,
            GenerateFormat::Mvt => TileFormat::Mvt,
        },
        encoding: options.encoding,
        batch_tiles: 256,
        window: threads * 4,
    }
}

/// What the tileset says about itself; the generator adds zooms, format and `vector_layers`.
fn metadata(options: &GenerateArgs) -> tilejson::TileJSON {
    let mut metadata = tilejson! { tiles: vec![] };
    metadata.name = options
        .output_file
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned());
    metadata.bounds = options.bbox;
    metadata.other.insert(
        "generator".to_owned(),
        format!("martin generate v{VERSION}").into(),
    );
    metadata
}

/// Discovers the table sources and picks the requested ones, each as one layer.
async fn select_sources(
    config: &Config,
    options: &GenerateArgs,
) -> GenerateResult<(PostgresPool, Vec<ScanLayer>)> {
    let resolver = IdResolver::new(RESERVED_KEYWORDS);
    let grids = TileGrids::resolve(&config.tile_grids)?;
    let mut pools = Vec::new();
    let mut tables: BTreeMap<String, (TableInfo, usize)> = BTreeMap::new();
    let mut functions = HashSet::new();
    for pg in &config.postgres {
        let builder =
            PostgresAutoDiscoveryBuilder::new(pg, resolver.clone(), config.cache.policy(), &grids)
                .await?;
        let (specs, _warnings) = builder.discover().await?;
        for (id, spec) in specs {
            match spec {
                SourceSpec::Table(info) => {
                    tables.insert(id, (info, pools.len()));
                }
                SourceSpec::Function(..) => {
                    functions.insert(id);
                }
            }
        }
        pools.push(builder.pool().clone());
    }
    let requested: Vec<String> = if options.sources.is_empty() {
        tables.keys().cloned().collect()
    } else {
        options.sources.clone()
    };

    let mut layers = Vec::with_capacity(requested.len());
    let mut pool = None;
    for id in requested {
        let Some((info, pool_idx)) = tables.get(&id) else {
            if functions.contains(&id) {
                return Err(GenerateError::FunctionSource(id));
            }
            return Err(GenerateError::UnknownSource(
                id,
                tables.keys().cloned().collect::<Vec<_>>().join(", "),
            ));
        };
        if let Some(grid) = info
            .tile_grid
            .as_deref()
            .filter(|g| *g != "WebMercatorQuad")
        {
            return Err(GenerateError::UnsupportedGrid(id, grid.to_owned()));
        }
        if pool.replace(*pool_idx).is_some_and(|p| p != *pool_idx) {
            return Err(GenerateError::MultipleDatabases);
        }
        let name = info.layer_id.clone().unwrap_or_else(|| id.clone());
        let min = info.minzoom.unwrap_or(0).max(options.min_zoom);
        let max = info.maxzoom.unwrap_or(u8::MAX).min(options.max_zoom);
        if min > max {
            return Err(GenerateError::NoZooms(
                id,
                options.min_zoom,
                options.max_zoom,
            ));
        }
        layers.push(ScanLayer {
            name,
            info: info.clone(),
            zooms: min..=max,
            bbox: options.bbox,
        });
    }
    let pool = pool.ok_or(GenerateError::NoSources)?;
    Ok((pools.swap_remove(pool), layers))
}

async fn report(progress: Arc<Progress>) {
    let mut interval = tokio::time::interval(PROGRESS_EVERY);
    interval.tick().await;
    loop {
        interval.tick().await;
        let features = progress.features.load(Ordering::Relaxed);
        if progress.writing.load(Ordering::Relaxed) {
            info!(
                features,
                tiles = progress.tiles.load(Ordering::Relaxed),
                "Rendered features and written tiles"
            );
        } else {
            info!(features, "Rendered features");
        }
    }
}

/// A failed or interrupted run leaves no partial tileset behind.
fn remove_output(path: &Path) {
    for path in [
        path.to_path_buf(),
        PathBuf::from(format!("{}-journal", path.display())),
    ] {
        if path.exists()
            && let Err(err) = std::fs::remove_file(&path)
        {
            warn!(path = %path.display(), error = %err, "Could not remove partial output");
        }
    }
}

fn parse_encoding(value: &str) -> Result<Encoding, String> {
    Encoding::parse(value)
        .ok_or_else(|| format!("unknown encoding `{value}`; use gzip, zstd, br, zlib or none"))
}
