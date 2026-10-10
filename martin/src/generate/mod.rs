//! `martin generate`: bulk tile generation from whole-table scans.

pub mod layers;
pub mod postgres;

use std::collections::{BTreeMap, HashSet};
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use martin_core::tiles::postgres::{PostgresError, PostgresPool, RetryTimeout};
use martin_tile_utils::{Encoding, WEB_MERCATOR_QUAD};
use martin_tilegen::{
    GenerateConfig, MbtilesSink, PmtilesSink, Progress, SortConfig, Summary, TileFormat,
    TileGenError, TileGenResult, generate,
};
use tilejson::{Bounds, tilejson};
#[cfg(unix)]
use tokio::signal::unix::{Signal, SignalKind, signal};
#[cfg(windows)]
use tokio::signal::windows::{CtrlBreak, CtrlC, ctrl_break, ctrl_c};
use tracing::{info, warn};

use self::layers::{LowerOptions, lower_table};
use self::postgres::{PgScanSource, ScanTable};
use crate::StartupError;
use crate::config::args::{
    Args, ArgsError, BoundsCalcType, ExtraArgs, MetaArgs, PostgresArgs, SrvArgs, parse_byte_size,
};
use crate::config::file::postgres::{
    DEFAULT_POOL_SIZE, PostgresAutoDiscoveryBuilder, SourceSpec, TableInfo,
};
use crate::config::file::{Config, ConfigFileError, OnInvalid, TileGrids, read_config};
use crate::config::primitives::IdResolver;
use crate::config::primitives::env::OsEnv;
use crate::cp::{MartinCpError, check_bboxes};
use crate::logging::LogFormat;
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
    pub sources: GenerateSourceArgs,
    #[command(flatten)]
    pub pg: Option<GeneratePgArgs>,
}

/// Where the tables come from: the subset of [`MetaArgs`] that `martin generate` uses.
#[derive(clap::Args, Debug, PartialEq)]
pub struct GenerateSourceArgs {
    /// Path to config file. If set, no connection strings are allowed.
    #[arg(short, long)]
    pub config: Option<PathBuf>,
    /// `PostgreSQL` connection strings, e.g. `postgres://...`
    pub connection: Vec<String>,
    /// Action to take when a source is found to be invalid during startup. [DEFAULT: abort]
    #[arg(long)]
    pub on_invalid: Option<OnInvalid>,
}

impl From<GenerateSourceArgs> for MetaArgs {
    fn from(args: GenerateSourceArgs) -> Self {
        Self {
            config: args.config,
            connection: args.connection,
            on_invalid: args.on_invalid,
            ..Self::default()
        }
    }
}

/// The subset of [`PostgresArgs`] that `martin generate` uses.
#[derive(clap::Args, Debug, PartialEq, Eq)]
pub struct GeneratePgArgs {
    /// Specify how bounds should be computed for the spatial PG tables. [DEFAULT: quick]
    #[arg(short = 'b', long)]
    pub auto_bounds: Option<BoundsCalcType>,
    /// Loads trusted root certificates from a file. The file should contain a sequence of PEM-formatted CA certificates.
    #[arg(long)]
    pub ca_root_file: Option<PathBuf>,
    /// If a spatial PG table has SRID 0, then this default SRID will be used as a fallback.
    #[arg(short, long)]
    pub default_srid: Option<i32>,
    #[arg(help = format!("Maximum Postgres connections pool size [DEFAULT: {DEFAULT_POOL_SIZE}]"), short, long)]
    pub pool_size: Option<NonZeroUsize>,
    /// How long the first PostgreSQL connection is retried before startup fails, a duration like `30s` or `infinite`. [DEFAULT: 30s]
    #[arg(long)]
    pub pg_retry_timeout: Option<RetryTimeout>,
    /// A file with a client SSL certificate.
    #[arg(long)]
    pub ssl_cert: Option<PathBuf>,
    /// A file with the key for the client SSL certificate.
    #[arg(long)]
    pub ssl_key: Option<PathBuf>,
}

impl From<GeneratePgArgs> for PostgresArgs {
    fn from(args: GeneratePgArgs) -> Self {
        Self {
            auto_bounds: args.auto_bounds,
            ca_root_file: args.ca_root_file,
            default_srid: args.default_srid,
            pool_size: args.pool_size,
            pg_retry_timeout: args.pg_retry_timeout,
            max_feature_count: None,
            ssl_cert: args.ssl_cert,
            ssl_key: args.ssl_key,
        }
    }
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
    /// Tile compression: `gzip`, `zlib` or `none` for `MBTiles`; `gzip`, `br`, `zstd` or `none` for MVT in `PMTiles`;
    /// only `none` for MLT in `PMTiles`. Defaults to `none` for MLT in `PMTiles`, otherwise `gzip`.
    #[arg(long, value_parser = parse_encoding)]
    pub encoding: Option<Encoding>,
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
    #[error("layer `{layer}`: {what} is not supported by martin generate yet")]
    Unsupported { layer: String, what: String },
    #[error("Sources come from more than one database; generate them separately")]
    MultipleDatabases,
    #[error("No table sources to generate")]
    NoSources,
    #[error("Source `{0}` has no zoom between {1} and {2}")]
    NoZooms(String, u8, u8),
    #[error("No layer has a zoom between {0} and {1}")]
    NoLayers(u8, u8),
    #[error("Martin cannot serve compressed MLT tiles from PMTiles; use `--encoding none`")]
    CompressedPmtilesMlt,
    #[error(
        "Martin cannot detect {0} compression in MBTiles; use `--encoding gzip`, `zlib` or `none`"
    )]
    UndetectableMbtilesEncoding(&'static str),
    #[error("Interrupted; removed the partial output {}", .0.display())]
    Interrupted(PathBuf),
    #[error("Could not listen for interrupts: {0}")]
    Signal(#[source] std::io::Error),
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

impl GenerateError {
    /// Renders the error for the terminal: config errors as diagnostics matching `log_format`, the rest with every cause.
    #[must_use]
    pub fn render_diagnostic_with(&self, log_format: LogFormat) -> String {
        if let Self::Startup(err) = self {
            return err.render_diagnostic_with(log_format);
        }
        let mut message = self.to_string();
        let mut source = std::error::Error::source(self);
        while let Some(err) = source {
            let cause = err.to_string();
            if !message.contains(&cause) {
                message = format!("{message}: {cause}");
            }
            source = err.source();
        }
        message
    }
}

pub type GenerateResult<T> = Result<T, GenerateError>;

pub async fn start(args: GeneratorArgs) -> GenerateResult<()> {
    info!(martin.version = %VERSION, "Starting Martin tile generator");
    let started = Instant::now();
    let options = args.generate;
    let encoding = resolve_encoding(&options)?;
    if let Some(bbox) = options.bbox {
        check_bboxes(&WEB_MERCATOR_QUAD, vec![bbox])?;
    }
    let mut config = match &args.sources.config {
        Some(path) => read_config(path, &OsEnv)?,
        None => Config::default(),
    };
    Args {
        command: None,
        meta: args.sources.into(),
        extras: ExtraArgs::default(),
        srv: SrvArgs::default(),
        pg: args.pg.map(PostgresArgs::from),
    }
    .merge_into_config(&mut config)?;
    config.finalize().await?;
    config.warn_unrecognized_keys();

    let (pool, tables) = select_sources(&config, &options).await?;
    let threads = options
        .threads
        .or_else(|| std::thread::available_parallelism().ok())
        .map_or(1, NonZeroUsize::get);
    let source = PgScanSource::new(pool, tables)?;
    let generate_config = generate_config(&options, threads, encoding);
    let metadata = metadata(&options);
    let output = options.output_file;
    let mut interrupt = Interrupt::register()?;
    let sink = {
        let (output, format) = (output.clone(), generate_config.format);
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
        () = interrupt.recv() => Err(GenerateError::Interrupted(output.clone())),
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
    Pmtiles(Box<PmtilesSink>),
}

impl Output {
    fn create(path: &Path, format: TileFormat, encoding: Encoding) -> TileGenResult<Self> {
        if is_pmtiles(path) {
            PmtilesSink::create(path, format, encoding).map(|sink| Self::Pmtiles(Box::new(sink)))
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
                generate(source, source.plan(), *sink, config, metadata, progress)
            }
        }
    }
}

fn is_pmtiles(path: &Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("pmtiles"))
}

/// The tile compression to write, refusing any that Martin could not serve the output with.
fn resolve_encoding(options: &GenerateArgs) -> GenerateResult<Encoding> {
    let pmtiles_mlt = is_pmtiles(&options.output_file) && options.format == GenerateFormat::Mlt;
    match options.encoding {
        None if pmtiles_mlt => Ok(Encoding::Uncompressed),
        None => Ok(Encoding::Gzip),
        Some(encoding) if pmtiles_mlt && encoding.is_encoded() => {
            Err(GenerateError::CompressedPmtilesMlt)
        }
        Some(encoding @ (Encoding::Brotli | Encoding::Zstd))
            if !is_pmtiles(&options.output_file) =>
        {
            Err(GenerateError::UndetectableMbtilesEncoding(
                encoding.compression().unwrap_or_default(),
            ))
        }
        Some(encoding) => Ok(encoding),
    }
}

fn generate_config(options: &GenerateArgs, threads: usize, encoding: Encoding) -> GenerateConfig {
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
        encoding,
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

/// Discovers the table sources and lowers the requested ones.
async fn select_sources(
    config: &Config,
    options: &GenerateArgs,
) -> GenerateResult<(PostgresPool, Vec<ScanTable>)> {
    let resolver = IdResolver::new(RESERVED_KEYWORDS);
    let grids = TileGrids::resolve(&config.tile_grids)?;
    let mut pools = Vec::new();
    let mut tables: BTreeMap<String, (TableInfo, usize)> = BTreeMap::new();
    let mut functions = HashSet::new();
    let mut warnings = Vec::new();
    for pg in &config.postgres {
        let builder =
            PostgresAutoDiscoveryBuilder::new(pg, resolver.clone(), config.cache.policy(), &grids)
                .await?;
        let (specs, pg_warnings) = builder.discover().await?;
        warnings.extend(pg_warnings);
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
    config
        .on_invalid
        .unwrap_or_default()
        .handle_tile_warnings(&warnings)?;
    let requested: Vec<String> = if options.sources.is_empty() {
        tables.keys().cloned().collect()
    } else {
        options.sources.clone()
    };

    let lower = LowerOptions {
        zooms: options.min_zoom..=options.max_zoom,
        bbox: options.bbox,
    };
    let mut scans = Vec::with_capacity(requested.len());
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
        scans.extend(lower_table(&id, info.clone(), &lower)?);
    }
    let pool = pool.ok_or(GenerateError::NoSources)?;
    if scans.is_empty() {
        return Err(GenerateError::NoLayers(options.min_zoom, options.max_zoom));
    }
    Ok((pools.swap_remove(pool), scans))
}

/// Ctrl-C and `SIGTERM` (Ctrl+Break on Windows), registered before the output exists so that neither can leave it behind.
struct Interrupt {
    #[cfg(unix)]
    interrupt: Signal,
    #[cfg(unix)]
    terminate: Signal,
    #[cfg(windows)]
    ctrl_c: CtrlC,
    #[cfg(windows)]
    ctrl_break: CtrlBreak,
}

impl Interrupt {
    fn register() -> GenerateResult<Self> {
        #[cfg(unix)]
        {
            Ok(Self {
                interrupt: signal(SignalKind::interrupt()).map_err(GenerateError::Signal)?,
                terminate: signal(SignalKind::terminate()).map_err(GenerateError::Signal)?,
            })
        }
        #[cfg(windows)]
        {
            Ok(Self {
                ctrl_c: ctrl_c().map_err(GenerateError::Signal)?,
                ctrl_break: ctrl_break().map_err(GenerateError::Signal)?,
            })
        }
    }

    async fn recv(&mut self) {
        #[cfg(unix)]
        tokio::select! {
            _ = self.interrupt.recv() => {}
            _ = self.terminate.recv() => {}
        }
        #[cfg(windows)]
        tokio::select! {
            _ = self.ctrl_c.recv() => {}
            _ = self.ctrl_break.recv() => {}
        }
    }
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
        PathBuf::from(format!("{}-wal", path.display())),
        PathBuf::from(format!("{}-shm", path.display())),
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
