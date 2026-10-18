//! `martin cp --mode bulk`: bulk tile generation from whole-table scans.

pub mod layers;
pub mod postgres;
pub mod serve;

use std::collections::{BTreeMap, HashSet};
use std::num::NonZeroUsize;
use std::ops::RangeInclusive;
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
#[cfg(unix)]
use tokio::signal::unix::{Signal, SignalKind, signal};
#[cfg(windows)]
use tokio::signal::windows::{CtrlBreak, CtrlC, ctrl_break, ctrl_c};
use tracing::{info, warn};

use self::layers::{LowerOptions, lower_table};
use self::postgres::{PgScanSource, ScanTable};
use crate::StartupError;
use crate::config::args::{Args, ArgsError, ExtraArgs, MetaArgs, PostgresArgs, SrvArgs};
use crate::config::file::postgres::{PostgresAutoDiscoveryBuilder, SourceSpec, TableInfo};
use crate::config::file::{Config, ConfigFileError, TileGrids, read_config};
use crate::config::primitives::IdResolver;
use crate::config::primitives::env::OsEnv;
use crate::cp::{CopierArgs, CopyArgs, CopyFormat, MartinCpError, check_bboxes};
use crate::logging::LogFormat;
use crate::srv::RESERVED_KEYWORDS;

const VERSION: &str = env!("CARGO_PKG_VERSION");
const PROGRESS_EVERY: Duration = Duration::from_secs(10);
/// Per-worker sort buffers are kept between these: smaller spills too many runs, larger cannot be indexed.
const BUFFER_BYTES: RangeInclusive<u64> = (16 << 20)..=(u32::MAX as u64);
const DEFAULT_MEMORY: u64 = 2 << 30;
const DEFAULT_MAX_ZOOM: u8 = 14;

/// What a bulk run renders and how, resolved from the `martin cp` arguments.
#[derive(Debug)]
struct GenerateOptions {
    sources: Vec<String>,
    output_file: PathBuf,
    format: TileFormat,
    encoding: Encoding,
    zooms: RangeInclusive<u8>,
    bbox: Option<Bounds>,
    memory: u64,
    temp_dirs: Vec<PathBuf>,
    threads: usize,
}

impl GenerateOptions {
    /// Takes the options bulk mode reads, and refuses the ones it would ignore.
    fn new(args: CopyArgs, meta: &MetaArgs, pg: Option<&PostgresArgs>) -> GenerateResult<Self> {
        let tile_only: Vec<&str> = [
            ("--url-query", args.url_query.is_some()),
            ("--mbtiles-type", args.mbt_type.is_some()),
            ("--on-duplicate", args.on_duplicate.is_some()),
            ("--concurrency", args.concurrency.is_some()),
            ("--skip-agg-tiles-hash", args.skip_agg_tiles_hash),
            ("--set-meta", !args.set_meta.is_empty()),
            ("--save-config", meta.save_config.is_some()),
            (
                "--max-feature-count",
                pg.is_some_and(|pg| pg.max_feature_count.is_some()),
            ),
        ]
        .into_iter()
        .filter_map(|(option, used)| used.then_some(option))
        .collect();
        if !tile_only.is_empty() {
            return Err(GenerateError::TileOnlyOptions(tile_only.join("`, `")));
        }
        let format = match args.format {
            None | Some(CopyFormat::MltV1) => TileFormat::Mlt,
            Some(CopyFormat::Mvt) => TileFormat::Mvt,
            #[cfg(feature = "unstable-mlt-v2")]
            Some(CopyFormat::MltV2) => return Err(GenerateError::MltV2),
        };
        let encoding = args
            .encoding
            .as_deref()
            .map(|value| {
                Encoding::parse(value).ok_or_else(|| GenerateError::BadEncoding(value.to_owned()))
            })
            .transpose()?;
        let encoding = resolve_encoding(&args.output_file, format, encoding)?;
        let bbox = match args.bbox.as_slice() {
            [] => None,
            [bbox] => Some(*bbox),
            _ => return Err(GenerateError::SeveralBboxes),
        };
        if let Some(bbox) = bbox {
            check_bboxes(&WEB_MERCATOR_QUAD, vec![bbox])?;
        }
        let threads = args
            .threads
            .or_else(|| std::thread::available_parallelism().ok())
            .map_or(1, NonZeroUsize::get);
        Ok(Self {
            sources: args
                .source
                .iter()
                .flat_map(|ids| ids.split(','))
                .map(str::to_owned)
                .collect(),
            output_file: args.output_file,
            format,
            encoding,
            zooms: zoom_range(args.min_zoom, args.max_zoom, args.zoom_levels)?,
            bbox,
            memory: args.memory.unwrap_or(DEFAULT_MEMORY),
            temp_dirs: if args.temp_dir.is_empty() {
                vec![std::env::temp_dir()]
            } else {
                args.temp_dir
            },
            threads,
        })
    }
}

/// The zooms to render: `--zoom-levels` without gaps, or `--min-zoom` to `--max-zoom`.
fn zoom_range(
    min_zoom: Option<u8>,
    max_zoom: Option<u8>,
    mut levels: Vec<u8>,
) -> GenerateResult<RangeInclusive<u8>> {
    levels.sort_unstable();
    levels.dedup();
    match (levels.first(), levels.last()) {
        (Some(&first), Some(&last)) if usize::from(last - first) + 1 == levels.len() => {
            Ok(first..=last)
        }
        (Some(_), _) => Err(GenerateError::ZoomGaps),
        _ => Ok(min_zoom.unwrap_or(0)..=max_zoom.unwrap_or(DEFAULT_MAX_ZOOM)),
    }
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
    #[error("`--mode bulk` does not read `{0}`")]
    TileOnlyOptions(String),
    #[error("`--mode bulk` takes at most one `--bbox`")]
    SeveralBboxes,
    #[error("`--mode bulk` renders a range of zooms; `--zoom-levels` must not skip a zoom")]
    ZoomGaps,
    #[cfg(feature = "unstable-mlt-v2")]
    #[error("`--mode bulk` cannot write `--format mlt2` yet")]
    MltV2,
    #[error("unknown encoding `{0}`; `--mode bulk` takes one of gzip, zlib, br, zstd or none")]
    BadEncoding(String),
    #[error("No table source matches `{0}`. Available table sources: {1}")]
    UnknownSource(String, String),
    #[error("Source `{0}` is a function source; only tables and views can be scanned")]
    FunctionSource(String),
    #[error("Source `{0}` uses tile grid `{1}`; `--mode bulk` supports Web Mercator only")]
    UnsupportedGrid(String, String),
    #[error("layer `{layer}`: {what} is not supported by `--mode bulk` yet")]
    Unsupported { layer: String, what: String },
    #[error("Sources come from more than one database; generate them separately")]
    MultipleDatabases,
    #[error("No table sources to generate")]
    NoSources,
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

/// Renders whole tables into the output file, as `martin cp --mode bulk`.
pub async fn start(args: CopierArgs) -> GenerateResult<()> {
    let options = GenerateOptions::new(args.copy, &args.meta, args.pg.as_ref())?;
    info!(martin.version = %VERSION, "Starting Martin tile generator");
    let started = Instant::now();
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

    let (pool, scans) = select_sources(&config, &options).await?;
    let source = PgScanSource::new(pool, scans, PgScanSource::PARTITION_BLOCKS).await?;
    let generate_config = generate_config(&options);
    let encoding = options.encoding;
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
    for failed in &summary.expr_errors {
        warn!(
            "Layer `{}`: `{}` failed {} times and was taken as null",
            failed.layer, failed.expr, failed.errors
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
        if is_pmtiles(path) {
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

fn is_pmtiles(path: &Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("pmtiles"))
}

/// The tile compression to write, refusing any that Martin could not serve the output with.
fn resolve_encoding(
    output_file: &Path,
    format: TileFormat,
    encoding: Option<Encoding>,
) -> GenerateResult<Encoding> {
    let pmtiles_mlt = is_pmtiles(output_file) && matches!(format, TileFormat::Mlt);
    match encoding {
        None if pmtiles_mlt => Ok(Encoding::Uncompressed),
        None => Ok(Encoding::Gzip),
        Some(encoding) if pmtiles_mlt && encoding.is_encoded() => {
            Err(GenerateError::CompressedPmtilesMlt)
        }
        Some(encoding @ (Encoding::Brotli | Encoding::Zstd)) if !is_pmtiles(output_file) => Err(
            GenerateError::UndetectableMbtilesEncoding(encoding.compression().unwrap_or_default()),
        ),
        Some(encoding) => Ok(encoding),
    }
}

fn generate_config(options: &GenerateOptions) -> GenerateConfig {
    let threads = options.threads;
    let per_thread =
        (options.memory / threads as u64).clamp(*BUFFER_BYTES.start(), *BUFFER_BYTES.end());
    GenerateConfig {
        threads,
        sort: SortConfig {
            temp_dirs: options.temp_dirs.clone(),
            buffer_bytes: usize::try_from(per_thread).unwrap_or(usize::MAX),
            max_fan_in: 256,
        },
        format: options.format,
        encoding: options.encoding,
        batch_tiles: 256,
        window: threads * 4,
    }
}

/// What the tileset says about itself; the generator adds zooms, format and `vector_layers`.
fn metadata(options: &GenerateOptions) -> tilejson::TileJSON {
    let mut metadata = tilejson! { tiles: vec![] };
    metadata.name = options
        .output_file
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned());
    metadata.bounds = options.bbox;
    metadata.other.insert(
        "generator".to_owned(),
        format!("martin cp v{VERSION} (bulk)").into(),
    );
    metadata
}

/// Discovers the table sources and lowers the requested ones.
async fn select_sources(
    config: &Config,
    options: &GenerateOptions,
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
        zooms: options.zooms.clone(),
        bounds: options.bbox,
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
        return Err(GenerateError::NoLayers(
            *options.zooms.start(),
            *options.zooms.end(),
        ));
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
