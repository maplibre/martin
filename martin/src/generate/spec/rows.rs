//! The rows on the paths through the layers: selected by the backend where it can and confirmed by the plan, else
//! found by a sweep over the table's rows.

use std::collections::BTreeMap;
use std::fmt::{self, Display, Formatter};
use std::ops::ControlFlow;
use std::time::Instant;

use indexmap::IndexMap;
use martin_tilegen::TileGenError;
use martin_tilegen::plan::{Branch, ColumnDef, Plan, TranslateError};
use martin_tilegen::props::Prop;
use martin_tilegen::source::{Crs, FeatureSource, Geometry, SourceError, SourceFeature};
use mlt_core::geo_types;
use serde_json::Value;
use wkt::ToWkt as _;

use crate::generate::GenerateResult;
use crate::generate::spec::backend::{Find, RowKey, Rows, TestBackend, TestTable};
use crate::generate::spec::file::{Case, IdColumns, Spec, yaml};
use crate::generate::spec::report::{Change, Look, Reporter, Status, Tone};

/// Why a path has the row it has, or has none.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RowSearch {
    /// The table has no key column to select a row by.
    NoKey,
    /// Row `0` of the spec's case on the path still takes it.
    Kept(RowKey),
    /// The backend selected the rows on the path, and row `key` of them takes it.
    Selected { rows: String, key: RowKey },
    /// The backend selected the rows on the path, and there are none.
    NoneSelected { rows: String },
    /// A sweep came across row `key` on the path, searched for by a sweep `because` of these misses.
    Swept { key: RowKey, because: Vec<Miss> },
    /// A sweep over `rows` rows came across none on the path, searched for by a sweep `because` of these misses.
    NoneSwept { rows: u64, because: Vec<Miss> },
}

/// Why the search for a path's row went on to a sweep.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Miss {
    /// Row `0` of the spec's case on the path is gone.
    SpecRowGone(RowKey),
    /// Row `0` of the spec's case on the path no longer takes it.
    SpecRowOffPath(RowKey),
    /// The backend cannot select rows.
    BackendCannotSelect,
    /// No condition selects the rows on the path.
    CannotSelect(TranslateError),
    /// The backend selected the rows on the path, but row `key` of them does not take it.
    SelectedOffPath { rows: String, key: RowKey },
    /// The backend failed to select the rows on the path.
    SelectFailed { rows: String, error: String },
}

impl Display for Miss {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::SpecRowGone(key) => write!(f, "row {key} of the spec is gone"),
            Self::SpecRowOffPath(key) => write!(f, "row {key} of the spec no longer takes the path"),
            Self::BackendCannotSelect => f.write_str("the backend cannot select rows"),
            Self::CannotSelect(reason) => write!(f, "no condition selects the rows, it {reason}"),
            Self::SelectedOffPath { rows, key } => {
                write!(f, "row {key} of the rows where {rows} does not take the path")
            }
            Self::SelectFailed { rows, error } => write!(f, "selecting the rows where {rows} failed: {error}"),
        }
    }
}

impl RowSearch {
    /// A row is on the path.
    const FOUND: Look = Look::new("FOUND", Tone::Green, "found", false);
    /// No row is on the path.
    const NO_ROW: Look = Look::new("NO ROW", Tone::Yellow, "no row", true);
    /// The table has no key column.
    const NO_KEY: Look = Look::new("NO KEY", Tone::Yellow, "no key", true);

    /// The key of the row on the path, if there is one.
    #[must_use]
    pub const fn key(&self) -> Option<RowKey> {
        match self {
            Self::Kept(key) | Self::Selected { key, .. } | Self::Swept { key, .. } => Some(*key),
            Self::NoKey | Self::NoneSelected { .. } | Self::NoneSwept { .. } => None,
        }
    }
}

impl Status for RowSearch {
    const ALL: &'static [Look] = &[Self::FOUND, Self::NO_ROW, Self::NO_KEY];

    fn look(&self) -> Look {
        match self {
            Self::Kept(_) | Self::Selected { .. } | Self::Swept { .. } => Self::FOUND,
            Self::NoneSelected { .. } | Self::NoneSwept { .. } => Self::NO_ROW,
            Self::NoKey => Self::NO_KEY,
        }
    }
}

/// A path and the row found for it.
#[derive(Clone, Debug)]
pub struct Found {
    pub source: String,
    pub branch: Branch,
    pub row: RowSearch,
}

impl Display for Found {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let branch = &self.branch;
        match &self.row {
            RowSearch::NoKey => write!(f, "{branch}  the source has no key column"),
            RowSearch::Kept(key) => write!(f, "{branch}  row {key}, as in the spec"),
            RowSearch::Selected { key, .. } => write!(f, "{branch}  row {key}"),
            RowSearch::NoneSelected { .. } => branch.fmt(f),
            RowSearch::Swept { key, .. } => write!(f, "{branch}  row {key}, swept"),
            RowSearch::NoneSwept { rows, because } => {
                write!(f, "{branch}  none of {rows} rows swept")?;
                for miss in because {
                    write!(f, ", {miss}")?;
                }
                Ok(())
            }
        }
    }
}

/// A row as bulk mode reads it, with the paths it takes.
#[derive(Clone, Debug, PartialEq)]
pub struct ReadRow {
    pub key: RowKey,
    /// The paths through the layers the row takes, as the plan evaluates it.
    pub paths: Vec<String>,
    /// The columns the layers read and the key, with the geometry as WKT in the coordinates of the scan.
    pub input: BTreeMap<String, Value>,
}

/// The paths through the layers, and the rows found on them.
pub struct Search {
    pub found: Vec<Found>,
    /// Each row found, in order of first appearance.
    pub rows: IndexMap<(String, RowKey), ReadRow>,
}

/// Finds a row on each path through the layers of `tables`: the row of `old`'s case on it while it still takes the
/// path, else one the backend selects that takes it, else the first of a sweep over at most `sweep_rows` rows.
/// Each row lists the paths it was found for first.
///
/// # Errors
///
/// If a table cannot be planned or read.
pub async fn find_rows<B: TestBackend>(
    backend: &B,
    tables: &[TestTable],
    old: Option<&Spec>,
    sweep_rows: u64,
    report: &Reporter,
) -> GenerateResult<Search> {
    let ids = id_columns(tables);
    let mut search = Search {
        found: Vec::new(),
        rows: IndexMap::new(),
    };
    for table in tables {
        let source = &table.source;
        let mut swept: Vec<(usize, Vec<Miss>)> = Vec::new();
        for branch in table.paths()? {
            report.working(format_args!("{source} {branch}"));
            let started = Instant::now();
            let kept = case_on(old, source, &branch).and_then(|case| case.id(&ids));
            let (row, read) = if table.key.is_none() {
                (Ok(RowSearch::NoKey), None)
            } else {
                search_row(backend, table, &branch, kept).await?
            };
            if let Some(read) = read {
                search.rows.entry((source.clone(), read.key)).or_insert(read);
            }
            let found = match row {
                Ok(row) => {
                    let found = Found {
                        source: source.clone(),
                        branch,
                        row,
                    };
                    report.line(&found.row, Some(started.elapsed()), source, &found);
                    found
                }
                Err(because) => {
                    swept.push((search.found.len(), because));
                    Found {
                        source: source.clone(),
                        branch,
                        row: RowSearch::NoneSwept {
                            rows: 0,
                            because: Vec::new(),
                        },
                    }
                }
            };
            search.found.push(found);
        }
        if swept.is_empty() {
            continue;
        }
        let wanted: Vec<String> = swept.iter().map(|(i, _)| search.found[*i].branch.to_string()).collect();
        report.working(format_args!(
            "{source}: sweeping up to {sweep_rows} rows for {} paths",
            wanted.len()
        ));
        let started = Instant::now();
        let (hits, seen) = sweep(backend, table, wanted, sweep_rows).await?;
        for ((i, because), hit) in swept.into_iter().zip(hits) {
            let found = &mut search.found[i];
            found.row = match hit {
                Some(row) => {
                    let key = row.key;
                    search.rows.entry((source.clone(), key)).or_insert(row);
                    RowSearch::Swept { key, because }
                }
                None => RowSearch::NoneSwept { rows: seen, because },
            };
            report.line(&found.row, Some(started.elapsed()), source, &*found);
        }
    }
    let Search { found, rows } = &mut search;
    for ((source, key), row) in rows.iter_mut() {
        let found_for: Vec<String> = found
            .iter()
            .filter(|f| f.source == *source && f.row.key() == Some(*key))
            .map(|f| f.branch.to_string())
            .collect();
        row.paths.sort_by_key(|path| !found_for.contains(path));
    }
    Ok(search)
}

/// The row on `branch` of `table` without a sweep: row `kept` of the spec while it still takes the path, else one the
/// backend selects; else the misses that leave the path to a sweep.
async fn search_row<B: TestBackend>(
    backend: &B,
    table: &TestTable,
    branch: &Branch,
    kept: Option<RowKey>,
) -> GenerateResult<(Result<RowSearch, Vec<Miss>>, Option<ReadRow>)> {
    let path = branch.to_string();
    let mut because = Vec::new();
    if let Some(key) = kept {
        match read_row(backend, table, key).await? {
            Some(row) if row.paths.contains(&path) => return Ok((Ok(RowSearch::Kept(key)), Some(row))),
            Some(_) => because.push(Miss::SpecRowOffPath(key)),
            None => because.push(Miss::SpecRowGone(key)),
        }
    }
    let condition = match &branch.rows {
        Ok(condition) => condition,
        Err(err) => {
            because.push(Miss::CannotSelect(err.reason.clone()));
            return Ok((Err(because), None));
        }
    };
    match backend.find(table, condition, branch.shape).await {
        Find::Unsupported => because.push(Miss::BackendCannotSelect),
        Find::Searched { rows, key: None } => return Ok((Ok(RowSearch::NoneSelected { rows }), None)),
        Find::Searched { rows, key: Some(key) } => match read_row(backend, table, key).await? {
            Some(row) if row.paths.contains(&path) => {
                return Ok((Ok(RowSearch::Selected { rows, key }), Some(row)));
            }
            Some(_) | None => because.push(Miss::SelectedOffPath { rows, key }),
        },
        Find::Failed { rows, error } => because.push(Miss::SelectFailed { rows, error }),
    }
    Ok((Err(because), None))
}

/// The key column of each table that has one.
#[must_use]
pub fn id_columns(tables: &[TestTable]) -> IdColumns {
    tables
        .iter()
        .filter_map(|table| Some((table.source.clone(), table.key.clone()?)))
        .collect()
}

/// The case of `old` on `source`'s `branch`.
fn case_on<'a>(old: Option<&'a Spec>, source: &str, branch: &Branch) -> Option<&'a Case> {
    let path = branch.to_string();
    old?.cases
        .iter()
        .find(|case| case.source == source && case.paths.contains(&path))
}

/// Row `key` of `table`, or `None` when it is gone.
///
/// # Errors
///
/// If the row cannot be read.
pub async fn read_row<B: TestBackend>(backend: &B, table: &TestTable, key: RowKey) -> GenerateResult<Option<ReadRow>> {
    let scan = backend.scan(table, Some(key), Rows::All).await?;
    let table = table.clone();
    tokio::task::spawn_blocking(move || {
        let plan = table.plan()?;
        let mut out = None;
        each_row(&scan, &plan, |crs, feature| {
            if feature.id != Some(key) {
                return Ok(ControlFlow::Continue(()));
            }
            let paths = paths_of(&plan, &table, crs, feature)?;
            out = Some(read(&plan, &table, key, feature, paths));
            Ok(ControlFlow::Break(()))
        })?;
        Ok(out)
    })
    .await?
}

/// The first row of a sweep over at most `limit` rows of `table` on each path of `wanted`, and how many rows it read.
async fn sweep<B: TestBackend>(
    backend: &B,
    table: &TestTable,
    wanted: Vec<String>,
    limit: u64,
) -> GenerateResult<(Vec<Option<ReadRow>>, u64)> {
    if limit == 0 {
        return Ok((vec![None; wanted.len()], 0));
    }
    let scan = backend.scan(table, None, Rows::All).await?;
    let table = table.clone();
    tokio::task::spawn_blocking(move || {
        let plan = table.plan()?;
        let mut hits: Vec<Option<ReadRow>> = vec![None; wanted.len()];
        let mut seen = 0;
        each_row(&scan, &plan, |crs, feature| {
            seen += 1;
            if let Some(key) = feature.id {
                let paths = paths_of(&plan, &table, crs, feature)?;
                let open = |(path, hit): (&String, &Option<ReadRow>)| hit.is_none() && paths.contains(path);
                if wanted.iter().zip(&hits).any(open) {
                    let row = read(&plan, &table, key, feature, paths);
                    for (path, hit) in wanted.iter().zip(&mut hits) {
                        if hit.is_none() && row.paths.contains(path) {
                            *hit = Some(row.clone());
                        }
                    }
                }
            }
            let done = seen >= limit || hits.iter().all(Option::is_some);
            Ok(if done {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            })
        })?;
        Ok((hits, seen))
    })
    .await?
}

/// Passes `visit` each row of `scan`, its geometry in the given coordinates, until it breaks.
fn each_row(
    scan: &impl FeatureSource,
    plan: &Plan,
    mut visit: impl FnMut(Crs, &SourceFeature) -> GenerateResult<ControlFlow<()>>,
) -> GenerateResult<()> {
    let mut failed = None;
    for partition in 0..scan.partitions() {
        let read = scan.read(partition, plan.table_keys(), &mut |batch| {
            for feature in &batch.features {
                match visit(batch.crs, feature) {
                    Ok(ControlFlow::Continue(())) => {}
                    Ok(ControlFlow::Break(())) => return Err(SourceError::Stopped),
                    Err(err) => {
                        failed = Some(err);
                        return Err(SourceError::Stopped);
                    }
                }
            }
            Ok(())
        });
        if let Some(err) = failed {
            return Err(err);
        }
        match read {
            Ok(()) => {}
            Err(SourceError::Stopped) => return Ok(()),
            Err(err) => return Err(TileGenError::from(err).into()),
        }
    }
    Ok(())
}

/// The paths `feature` of `table` takes, its geometry in `crs`.
fn paths_of(plan: &Plan, table: &TestTable, crs: Crs, feature: &SourceFeature) -> GenerateResult<Vec<String>> {
    Ok(plan
        .paths_of(0, &table.known, crs, feature)?
        .iter()
        .map(ToString::to_string)
        .collect())
}

/// `feature` of `table` with key `key` as a row on `paths`.
fn read(plan: &Plan, table: &TestTable, key: RowKey, feature: &SourceFeature, paths: Vec<String>) -> ReadRow {
    let mut input: BTreeMap<String, Value> = table
        .def
        .columns
        .iter()
        .filter_map(|column| match column {
            ColumnDef::Value(name, _) if plan.columns(0).contains(name) => Some((name.clone(), Value::Null)),
            ColumnDef::Value(..) | ColumnDef::Keys(_) => None,
        })
        .collect();
    let keys = &plan.table_keys()[0];
    for (id, prop) in &feature.props {
        if let Some(name) = keys.name(*id) {
            input.insert(name, prop_value(prop));
        }
    }
    if let Some(column) = &table.key {
        input.insert(column.clone(), Value::from(key));
    }
    let geometry = wkt(&feature.geometry).map_or(Value::Null, Value::String);
    input.insert("geometry".to_owned(), geometry);
    ReadRow { key, paths, input }
}

/// A property as JSON.
fn prop_value<S: AsRef<str>>(prop: &Prop<S>) -> Value {
    match prop {
        Prop::Bool(v) => Value::from(*v),
        Prop::I64(v) => Value::from(*v),
        Prop::F32(v) => Value::from(f64::from(*v)),
        Prop::F64(v) => Value::from(*v),
        Prop::Str(v) => Value::from(v.as_ref()),
    }
}

/// `geometry` as WKT, or `None` when it cannot be decoded.
fn wkt(geometry: &Geometry) -> Option<String> {
    let mut geometry = geometry.clone();
    if !geometry.decode_wkb().ok()? {
        return None;
    }
    let geometry: geo_types::Geometry<f64> = match geometry {
        Geometry::Point(point) => geo_types::Point(point).into(),
        Geometry::Line(line) => line.into(),
        Geometry::Polygon(polygon) => polygon.into(),
        Geometry::Points(points) => geo_types::MultiPoint(points.into_iter().map(geo_types::Point).collect()).into(),
        Geometry::Lines(lines) => geo_types::MultiLineString(lines).into(),
        Geometry::Polygons(polygons) => geo_types::MultiPolygon(polygons).into(),
        Geometry::Wkb(_) => return None,
    };
    Some(geometry.wkt_string())
}

/// The input of `case` and its row now, as YAML, when they differ.
#[must_use]
pub fn row_change(case: &Case, now: Option<&ReadRow>) -> Option<Change> {
    let now = now.map(|row| &row.input);
    (now != Some(&case.input)).then(|| Change {
        old: yaml(&case.input),
        new: now.map_or_else(|| "(the row is gone)\n".to_owned(), yaml),
    })
}
