//! `martin test`: specs of what the layers of table sources make of real rows, generated and checked.

pub mod backend;
pub mod file;
pub mod memory;
pub mod postgres;
pub mod render;
pub mod report;
pub mod review;
pub mod rows;

use std::path::PathBuf;
use std::time::Instant;

use crate::generate::spec::backend::{TestBackend, TestTable};
use crate::generate::spec::file::{Case, CaseName, IdColumns, Spec, output_yaml, read_spec, write_spec};
use crate::generate::spec::postgres::Postgres;
use crate::generate::spec::render::{Served, render};
use crate::generate::spec::report::{Change, Look, Reporter, Status, Tone, tally};
use crate::generate::spec::review::{Review, Reviewed};
use crate::generate::spec::rows::{Search, find_rows, id_columns, read_row, row_change};
use crate::generate::{GenerateError, GenerateResult};

/// The max zoom of a new spec.
const DEFAULT_MAX_ZOOM: u8 = 14;

/// How many rows a sweep reads per table at most, unless `--sweep-rows` says otherwise.
const DEFAULT_SWEEP_ROWS: u64 = 100_000;

/// `martin test` arguments.
#[derive(clap::Args, Debug, PartialEq)]
pub struct TestArgs {
    #[command(subcommand)]
    pub action: TestAction,
}

/// What `martin test` does.
#[derive(clap::Subcommand, Debug, PartialEq)]
pub enum TestAction {
    /// Find a row on every path through the layers, render each, and write how they differ from the spec
    Generate(GenerateArgs),
    /// Render every case of a spec and compare
    Run(RunArgs),
}

/// `martin test generate` arguments.
#[derive(clap::Args, Debug, PartialEq)]
pub struct GenerateArgs {
    /// Configuration file with the table sources
    #[arg(short, long)]
    pub config: PathBuf,
    /// Spec file to compare with and write
    #[arg(short, long)]
    pub output: PathBuf,
    /// Highest zoom of every case, the spec's if it exists, else 14
    #[arg(long)]
    pub max_zoom: Option<u8>,
    /// Write every case as generated
    #[arg(long)]
    pub accept: bool,
    /// Rows to read per table at most when looking for rows on the paths the database cannot select
    #[arg(long, default_value_t = DEFAULT_SWEEP_ROWS)]
    pub sweep_rows: u64,
}

/// `martin test run` arguments.
#[derive(clap::Args, Debug, PartialEq)]
pub struct RunArgs {
    /// Configuration file with the table sources
    #[arg(short, long)]
    pub config: PathBuf,
    /// Spec file to check
    pub spec: PathBuf,
    /// Serve each source up to its own `maxzoom` instead of the spec's
    #[arg(long)]
    pub serve_own_max_zoom: bool,
}

/// Runs `martin test`, `Ok(false)` when a case fails or `generate` has changes it may not write.
///
/// # Errors
///
/// If the configuration, the database or the spec cannot be read.
pub async fn start(args: TestArgs) -> GenerateResult<bool> {
    match args.action {
        TestAction::Generate(args) => generate_spec(args).await,
        TestAction::Run(args) => run_spec(args).await,
    }
}

/// The table of source `source`.
fn table_of<'a>(tables: &'a [TestTable], source: &str) -> GenerateResult<&'a TestTable> {
    tables.iter().find(|t| t.source == source).ok_or_else(|| {
        let known = tables.iter().map(|t| t.source.as_str()).collect::<Vec<_>>();
        GenerateError::UnknownSource(source.to_owned(), known.join(", "))
    })
}

/// One case per row found, with what bulk mode renders of it and how serving it differs, against `spec`.
async fn cases<B: TestBackend>(
    backend: &B,
    tables: &[TestTable],
    search: &Search,
    max_zoom: u8,
    spec: Option<&Spec>,
    ids: &IdColumns,
    report: &Reporter,
) -> GenerateResult<Vec<Reviewed>> {
    let mut out = Vec::new();
    for ((source, key), row) in &search.rows {
        let name = format!("{source} {key}");
        report.working(&name);
        let started = Instant::now();
        let rendered = render(backend, table_of(tables, source)?, *key, Some(max_zoom)).await?;
        let case = Case {
            source: source.clone(),
            paths: row.paths.clone(),
            input: row.input.clone(),
            output: rendered.output,
        };
        let reviewed = Reviewed::new(case, rendered.served, spec, ids);
        report.line(&reviewed.status, Some(started.elapsed()), &name, reviewed.case.path());
        report_served(report, &name, &reviewed.served);
        out.push(reviewed);
    }
    Ok(out)
}

/// How serving a case differs from bulk mode, if it does.
fn report_served(report: &Reporter, name: &str, served: &Served) {
    if !served.diffs.is_empty() {
        report.block(Tone::Red, "SERVED", name, served);
    }
}

/// Finds a row on each path through the layers of the backend's tables at zooms 0 to `max_zoom`, sweeping at most
/// `sweep_rows` rows of a table, renders each row, and reviews the cases against `old`.
///
/// # Errors
///
/// If a table cannot be planned, read or rendered.
pub async fn review<B: TestBackend>(
    backend: &B,
    max_zoom: u8,
    old: Option<&Spec>,
    sweep_rows: u64,
    report: &Reporter,
) -> GenerateResult<Review> {
    let tables = backend.tables(max_zoom)?;
    let mut count = 0;
    for table in &tables {
        count += table.paths()?.len();
    }
    report.start(
        "Searching",
        format_args!(
            "{count} paths through the layers of {} sources, max zoom {max_zoom}",
            tables.len()
        ),
        count,
    );
    let search = find_rows(backend, &tables, old, sweep_rows, report).await?;
    let count = search.rows.len();
    report.start(
        "Rendering",
        format_args!("{count} cases in bulk mode and served"),
        count,
    );
    let ids = id_columns(&tables);
    let cases = cases(backend, &tables, &search, max_zoom, old, &ids, report).await?;
    let review = Review::new(search.found, cases, old, max_zoom, ids);
    for r in review.gone() {
        report.line(&r.status, None, &review.name(&r.case).to_string(), r.case.path());
    }
    Ok(review)
}

/// `martin test generate`.
async fn generate_spec(args: GenerateArgs) -> GenerateResult<bool> {
    let started = Instant::now();
    let report = Reporter::new();
    let output = &args.output;
    let old = output.exists().then(|| read_spec(output)).transpose()?;
    let max_zoom = args
        .max_zoom
        .or(old.as_ref().map(|s| s.max_zoom))
        .unwrap_or(DEFAULT_MAX_ZOOM);
    let backend = Postgres::load(&args.config).await?;
    let mut review = review(&backend, max_zoom, old.as_ref(), args.sweep_rows, &report).await?;
    report.summary(started.elapsed(), review.summary());
    let pending = review.pending().count();
    if pending > 0 && !args.accept && old.is_some() {
        for r in review.pending() {
            let look = r.status.look();
            report.block(
                look.tone,
                look.label,
                &review.name(&r.case).to_string(),
                report.diff(("spec", "now"), &r.diff()),
            );
        }
        report.error(format_args!(
            "{pending} cases differ from {}, pass --accept to write them",
            output.display()
        ));
        return Ok(false);
    }
    review.accept_all();
    let spec = review.spec();
    if old.as_ref() == Some(&spec) {
        report.note(format_args!("{} is up to date", output.display()));
    } else {
        write_spec(output, &spec)?;
        report.note(format_args!("Wrote {}", output.display()));
    }
    Ok(true)
}

/// How a case of a spec runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunStatus {
    Pass,
    Fail,
    ModesDiffer,
    RowChanged,
}

impl RunStatus {
    /// The case renders as the spec says.
    const PASS: Look = Look::new("PASS", Tone::Green, "passed", false);
    /// The case renders other than the spec says.
    const FAIL: Look = Look::new("FAIL", Tone::Red, "failed", true);
    /// The server renders the case other than bulk mode does.
    const MODES_DIFFER: Look = Look::new("MODES DIFFER", Tone::Red, "modes differ", true);
    /// The case's row changed since the case was written.
    const ROW_CHANGED: Look = Look::new("ROW CHANGED", Tone::Yellow, "rows changed", true);
}

impl Status for RunStatus {
    const ALL: &'static [Look] = &[Self::PASS, Self::FAIL, Self::MODES_DIFFER, Self::ROW_CHANGED];

    fn look(&self) -> Look {
        match self {
            Self::Pass => Self::PASS,
            Self::Fail => Self::FAIL,
            Self::ModesDiffer => Self::MODES_DIFFER,
            Self::RowChanged => Self::ROW_CHANGED,
        }
    }
}

/// What one case renders to now.
pub struct Outcome {
    /// The row as in the case's input and as it is now, as YAML, when it changed.
    pub row: Option<Change>,
    /// The spec's output and bulk mode's now, as YAML, when they differ.
    pub output: Option<Change>,
    /// How serving the row differs from bulk mode.
    pub served: Served,
}

impl Outcome {
    /// The status, with a changed row before a changed output.
    #[must_use]
    pub const fn status(&self) -> RunStatus {
        if self.row.is_some() {
            RunStatus::RowChanged
        } else if self.output.is_some() {
            RunStatus::Fail
        } else if !self.served.diffs.is_empty() {
            RunStatus::ModesDiffer
        } else {
            RunStatus::Pass
        }
    }
}

/// Renders `case` of one of `tables` in bulk mode and served, the server up to `max_zoom` unless
/// `serve_own_max_zoom`, and compares.
///
/// # Errors
///
/// If the case has no key in its input, or cannot be rendered.
pub async fn check<B: TestBackend>(
    backend: &B,
    tables: &[TestTable],
    case: &Case,
    max_zoom: u8,
    serve_own_max_zoom: bool,
) -> GenerateResult<Outcome> {
    let table = table_of(tables, &case.source)?;
    let key = case
        .id(&id_columns(tables))
        .ok_or_else(|| GenerateError::CaseWithoutId(case.source.clone()))?;
    let now = read_row(backend, table, key).await?;
    let rendered = render(backend, table, key, (!serve_own_max_zoom).then_some(max_zoom)).await?;
    Ok(Outcome {
        row: row_change(case, now.as_ref()),
        output: (rendered.output != case.output).then(|| Change {
            old: output_yaml(&case.output),
            new: output_yaml(&rendered.output),
        }),
        served: rendered.served,
    })
}

/// `martin test run`.
async fn run_spec(args: RunArgs) -> GenerateResult<bool> {
    let started = Instant::now();
    let report = Reporter::new();
    let backend = Postgres::load(&args.config).await?;
    let spec = read_spec(&args.spec)?;
    let tables = backend.tables(spec.max_zoom)?;
    report.start(
        "Starting",
        format_args!(
            "{} cases of {} with {}, max zoom {}",
            spec.cases.len(),
            args.spec.display(),
            args.config.display(),
            spec.max_zoom
        ),
        spec.cases.len(),
    );
    let ids = id_columns(&tables);
    let mut statuses = Vec::with_capacity(spec.cases.len());
    for case in &spec.cases {
        let name = CaseName(case, &ids).to_string();
        report.working(&name);
        let case_started = Instant::now();
        let outcome = check(&backend, &tables, case, spec.max_zoom, args.serve_own_max_zoom).await?;
        let status = outcome.status();
        report.line(&status, Some(case_started.elapsed()), &name, case.path());
        if let Some(change) = &outcome.row {
            report.block(Tone::Yellow, "ROW", &name, report.diff(("input", "now"), change));
        }
        if let Some(change) = &outcome.output {
            report.block(
                Tone::Red,
                "OUTPUT",
                &name,
                report.diff(("spec", "bulk mode now"), change),
            );
        }
        report_served(&report, &name, &outcome.served);
        statuses.push(status);
    }
    report.summary(
        started.elapsed(),
        format_args!("{} cases run: {}", spec.cases.len(), tally(statuses.iter())),
    );
    let failed = statuses.iter().filter(|s| **s != RunStatus::Pass).count();
    if failed > 0 {
        report.error(format_args!("{failed} of {} cases did not pass", spec.cases.len()));
    }
    if statuses
        .iter()
        .any(|s| matches!(s, RunStatus::Fail | RunStatus::RowChanged))
    {
        report.hint(format_args!(
            "to see the new output and accept it: martin test generate -c {} -o {}",
            args.config.display(),
            args.spec.display()
        ));
    }
    Ok(failed == 0)
}

#[cfg(test)]
mod tests {
    use indoc::indoc;

    use martin_tilegen::plan::{Condition, Shape};
    use martin_tilegen::source::MemorySource;

    use super::*;
    use crate::config::file::postgres::TableInfo;
    use crate::generate::spec::backend::{Find, RowKey, Rows};
    use crate::generate::spec::file::yaml;
    use crate::generate::spec::memory::{Memory, MemoryTable};
    use crate::generate::spec::postgres::test_table;
    use crate::generate::spec::rows::RowSearch;

    /// Rows held in memory, whose backend answers every search with `find`.
    struct Selects {
        memory: Memory,
        find: Find,
    }

    impl TestBackend for Selects {
        type Scan = MemorySource;

        fn tables(&self, max_zoom: u8) -> GenerateResult<Vec<TestTable>> {
            self.memory.tables(max_zoom)
        }

        async fn scan(&self, table: &TestTable, key: Option<RowKey>, rows: Rows) -> GenerateResult<MemorySource> {
            self.memory.scan(table, key, rows).await
        }

        #[expect(clippy::unused_async_trait_impl, reason = "the answer is fixed, a real search waits")]
        async fn find(&self, _table: &TestTable, _rows: &Condition, _shape: Option<Shape>) -> Find {
            self.find.clone()
        }
    }

    /// The status of `outcome` and its diffs, as `martin test run` prints them.
    fn shown(outcome: &Outcome) -> String {
        let report = Reporter::new();
        let mut out = format!("{:?}\n", outcome.status());
        if let Some(change) = &outcome.row {
            out.push_str(&report.diff(("input", "now"), change).to_string());
        }
        if let Some(change) = &outcome.output {
            out.push_str(&report.diff(("spec", "bulk mode now"), change).to_string());
        }
        out
    }

    #[tokio::test]
    async fn a_sweep_finds_the_row_when_the_backend_cannot_select() {
        let info: TableInfo = serde_saphyr::from_str(indoc! {"
            schema: public
            table: places
            srid: 4326
            geometry_column: geom
            geometry_type: POINT
            id_column: id
            properties:
              id: int8
            layers:
              places: {}
        "})
        .expect("parses");
        let rows = serde_saphyr::from_str(indoc! {"
            - { id: 1, geometry: 'POINT(16.37 48.20)' }
        "})
        .expect("parses");
        let memory = Memory {
            tables: vec![MemoryTable {
                table: test_table("places", &info, 14).expect("lowers").expect("has zooms"),
                rows,
            }],
        };
        let review = review(&memory, 14, None, 100, &Reporter::new()).await.expect("reviews");
        let found: Vec<(String, RowSearch)> = review
            .found
            .iter()
            .map(|found| (found.branch.to_string(), found.row.clone()))
            .collect();
        insta::assert_debug_snapshot!(found, @r#"
        [
            (
                "places: every feature",
                Swept {
                    key: 1,
                    because: [
                        BackendCannotSelect,
                    ],
                },
            ),
        ]
        "#);
    }

    #[tokio::test]
    async fn a_sweep_over_an_empty_table_finds_no_row() {
        let info: TableInfo = serde_saphyr::from_str(indoc! {"
            schema: public
            table: places
            srid: 4326
            geometry_column: geom
            geometry_type: POINT
            id_column: id
            properties:
              id: int8
            layers:
              places: {}
        "})
        .expect("parses");
        let rows = Vec::new();
        let memory = Memory {
            tables: vec![MemoryTable {
                table: test_table("places", &info, 14).expect("lowers").expect("has zooms"),
                rows,
            }],
        };
        let review = review(&memory, 14, None, 100, &Reporter::new()).await.expect("reviews");
        let found: Vec<(String, RowSearch)> = review
            .found
            .iter()
            .map(|found| (found.branch.to_string(), found.row.clone()))
            .collect();
        insta::assert_debug_snapshot!(found, @r#"
        [
            (
                "places: every feature",
                NoneSwept {
                    rows: 0,
                    because: [
                        BackendCannotSelect,
                    ],
                },
            ),
        ]
        "#);
    }

    #[tokio::test]
    async fn a_sweep_reads_no_more_rows_than_its_limit() {
        let info: TableInfo = serde_saphyr::from_str(indoc! {"
            schema: public
            table: places
            srid: 4326
            geometry_column: geom
            geometry_type: POINT
            id_column: id
            properties:
              id: int8
              population: int8
            layers:
              places:
                where: population > 1000
        "})
        .expect("parses");
        let rows = serde_saphyr::from_str(indoc! {"
            - { id: 1, population: 5000, geometry: 'POINT(16.37 48.20)' }
            - { id: 2, population: 10, geometry: 'POINT(16.38 48.21)' }
        "})
        .expect("parses");
        let memory = Memory {
            tables: vec![MemoryTable {
                table: test_table("places", &info, 14).expect("lowers").expect("has zooms"),
                rows,
            }],
        };
        let review = review(&memory, 14, None, 1, &Reporter::new()).await.expect("reviews");
        let found: Vec<(String, RowSearch)> = review
            .found
            .iter()
            .map(|found| (found.branch.to_string(), found.row.clone()))
            .collect();
        insta::assert_debug_snapshot!(found, @r#"
        [
            (
                "places: kept by `where: population > 1000`",
                Swept {
                    key: 1,
                    because: [
                        BackendCannotSelect,
                    ],
                },
            ),
            (
                "places: dropped, `where: population > 1000` is false",
                NoneSwept {
                    rows: 1,
                    because: [
                        BackendCannotSelect,
                    ],
                },
            ),
            (
                "places: dropped, `population` is null in `where: population > 1000`",
                NoneSwept {
                    rows: 1,
                    because: [
                        BackendCannotSelect,
                    ],
                },
            ),
        ]
        "#);
    }

    #[tokio::test]
    async fn a_sweep_limited_to_no_rows_reads_none() {
        let info: TableInfo = serde_saphyr::from_str(indoc! {"
            schema: public
            table: places
            srid: 4326
            geometry_column: geom
            geometry_type: POINT
            id_column: id
            properties:
              id: int8
            layers:
              places: {}
        "})
        .expect("parses");
        let rows = serde_saphyr::from_str(indoc! {"
            - { id: 1, geometry: 'POINT(16.37 48.20)' }
        "})
        .expect("parses");
        let memory = Memory {
            tables: vec![MemoryTable {
                table: test_table("places", &info, 14).expect("lowers").expect("has zooms"),
                rows,
            }],
        };
        let review = review(&memory, 14, None, 0, &Reporter::new()).await.expect("reviews");
        let found: Vec<(String, RowSearch)> = review
            .found
            .iter()
            .map(|found| (found.branch.to_string(), found.row.clone()))
            .collect();
        insta::assert_debug_snapshot!((found, review.cases.len()), @r#"
        (
            [
                (
                    "places: every feature",
                    NoneSwept {
                        rows: 0,
                        because: [
                            BackendCannotSelect,
                        ],
                    },
                ),
            ],
            0,
        )
        "#);
    }

    #[tokio::test]
    async fn the_row_the_backend_selects_is_taken_when_it_takes_the_path() {
        let info: TableInfo = serde_saphyr::from_str(indoc! {"
            schema: public
            table: places
            srid: 4326
            geometry_column: geom
            geometry_type: POINT
            id_column: id
            properties:
              id: int8
            layers:
              places: {}
        "})
        .expect("parses");
        let rows = serde_saphyr::from_str(indoc! {"
            - { id: 1, geometry: 'POINT(16.37 48.20)' }
        "})
        .expect("parses");
        let backend = Selects {
            memory: Memory {
                tables: vec![MemoryTable {
                    table: test_table("places", &info, 14).expect("lowers").expect("has zooms"),
                    rows,
                }],
            },
            find: Find::Searched {
                rows: "TRUE".to_owned(),
                key: Some(1),
            },
        };
        let review = review(&backend, 14, None, 100, &Reporter::new())
            .await
            .expect("reviews");
        let found: Vec<(String, RowSearch)> = review
            .found
            .iter()
            .map(|found| (found.branch.to_string(), found.row.clone()))
            .collect();
        insta::assert_debug_snapshot!(found, @r#"
        [
            (
                "places: every feature",
                Selected {
                    rows: "TRUE",
                    key: 1,
                },
            ),
        ]
        "#);
    }

    #[tokio::test]
    async fn a_path_the_backend_selects_no_row_for_is_not_swept() {
        let info: TableInfo = serde_saphyr::from_str(indoc! {"
            schema: public
            table: places
            srid: 4326
            geometry_column: geom
            geometry_type: POINT
            id_column: id
            properties:
              id: int8
            layers:
              places: {}
        "})
        .expect("parses");
        let rows = serde_saphyr::from_str(indoc! {"
            - { id: 1, geometry: 'POINT(16.37 48.20)' }
        "})
        .expect("parses");
        let backend = Selects {
            memory: Memory {
                tables: vec![MemoryTable {
                    table: test_table("places", &info, 14).expect("lowers").expect("has zooms"),
                    rows,
                }],
            },
            find: Find::Searched {
                rows: "FALSE".to_owned(),
                key: None,
            },
        };
        let review = review(&backend, 14, None, 100, &Reporter::new())
            .await
            .expect("reviews");
        let found: Vec<(String, RowSearch)> = review
            .found
            .iter()
            .map(|found| (found.branch.to_string(), found.row.clone()))
            .collect();
        insta::assert_debug_snapshot!(found, @r#"
        [
            (
                "places: every feature",
                NoneSelected {
                    rows: "FALSE",
                },
            ),
        ]
        "#);
    }

    #[tokio::test]
    async fn a_selected_row_off_the_path_leaves_the_path_to_a_sweep() {
        let info: TableInfo = serde_saphyr::from_str(indoc! {"
            schema: public
            table: places
            srid: 4326
            geometry_column: geom
            geometry_type: POINT
            id_column: id
            properties:
              id: int8
              population: int8
            layers:
              places:
                where: population > 1000
        "})
        .expect("parses");
        let rows = serde_saphyr::from_str(indoc! {"
            - { id: 1, population: 5000, geometry: 'POINT(16.37 48.20)' }
            - { id: 2, population: 10, geometry: 'POINT(16.38 48.21)' }
        "})
        .expect("parses");
        let backend = Selects {
            memory: Memory {
                tables: vec![MemoryTable {
                    table: test_table("places", &info, 14).expect("lowers").expect("has zooms"),
                    rows,
                }],
            },
            find: Find::Searched {
                rows: "TRUE".to_owned(),
                key: Some(1),
            },
        };
        let review = review(&backend, 14, None, 100, &Reporter::new())
            .await
            .expect("reviews");
        let found: Vec<(String, RowSearch)> = review
            .found
            .iter()
            .map(|found| (found.branch.to_string(), found.row.clone()))
            .collect();
        insta::assert_debug_snapshot!(found, @r#"
        [
            (
                "places: kept by `where: population > 1000`",
                Selected {
                    rows: "TRUE",
                    key: 1,
                },
            ),
            (
                "places: dropped, `where: population > 1000` is false",
                Swept {
                    key: 2,
                    because: [
                        SelectedOffPath {
                            rows: "TRUE",
                            key: 1,
                        },
                    ],
                },
            ),
            (
                "places: dropped, `population` is null in `where: population > 1000`",
                NoneSwept {
                    rows: 2,
                    because: [
                        SelectedOffPath {
                            rows: "TRUE",
                            key: 1,
                        },
                    ],
                },
            ),
        ]
        "#);
    }

    #[tokio::test]
    async fn a_failed_select_leaves_the_path_to_a_sweep() {
        let info: TableInfo = serde_saphyr::from_str(indoc! {"
            schema: public
            table: places
            srid: 4326
            geometry_column: geom
            geometry_type: POINT
            id_column: id
            properties:
              id: int8
            layers:
              places: {}
        "})
        .expect("parses");
        let rows = serde_saphyr::from_str(indoc! {"
            - { id: 1, geometry: 'POINT(16.37 48.20)' }
        "})
        .expect("parses");
        let backend = Selects {
            memory: Memory {
                tables: vec![MemoryTable {
                    table: test_table("places", &info, 14).expect("lowers").expect("has zooms"),
                    rows,
                }],
            },
            find: Find::Failed {
                rows: "TRUE".to_owned(),
                error: "canceling statement due to statement timeout".to_owned(),
            },
        };
        let review = review(&backend, 14, None, 100, &Reporter::new())
            .await
            .expect("reviews");
        let found: Vec<(String, RowSearch)> = review
            .found
            .iter()
            .map(|found| (found.branch.to_string(), found.row.clone()))
            .collect();
        insta::assert_debug_snapshot!(found, @r#"
        [
            (
                "places: every feature",
                Swept {
                    key: 1,
                    because: [
                        SelectFailed {
                            rows: "TRUE",
                            error: "canceling statement due to statement timeout",
                        },
                    ],
                },
            ),
        ]
        "#);
    }

    #[tokio::test]
    async fn a_path_no_condition_selects_is_swept_without_asking_the_backend() {
        let info: TableInfo = serde_saphyr::from_str(indoc! {"
            schema: public
            table: places
            srid: 4326
            geometry_column: geom
            geometry_type: POINT
            id_column: id
            properties:
              id: int8
              name: text
            layers:
              places:
                attributes:
                  short: \"name + '!' == 'A!' ? 'A' : name\"
        "})
        .expect("parses");
        let rows = serde_saphyr::from_str(indoc! {"
            - { id: 1, name: A, geometry: 'POINT(16.37 48.20)' }
            - { id: 2, name: B, geometry: 'POINT(16.38 48.21)' }
        "})
        .expect("parses");
        let backend = Selects {
            memory: Memory {
                tables: vec![MemoryTable {
                    table: test_table("places", &info, 14).expect("lowers").expect("has zooms"),
                    rows,
                }],
            },
            find: Find::Searched {
                rows: "TRUE".to_owned(),
                key: Some(1),
            },
        };
        let review = review(&backend, 14, None, 100, &Reporter::new())
            .await
            .expect("reviews");
        let found: Vec<(String, RowSearch)> = review
            .found
            .iter()
            .map(|found| (found.branch.to_string(), found.row.clone()))
            .collect();
        insta::assert_debug_snapshot!(found, @r#"
        [
            (
                "places: attributes.short 'A', `(name + '!') == 'A!'` is true",
                Swept {
                    key: 1,
                    because: [
                        CannotSelect(
                            Function(
                                "+",
                            ),
                        ),
                    ],
                },
            ),
            (
                "places: attributes.short `name`, `(name + '!') == 'A!'` is false",
                Swept {
                    key: 2,
                    because: [
                        CannotSelect(
                            Function(
                                "+",
                            ),
                        ),
                    ],
                },
            ),
            (
                "places: attributes.short, `name` is null in `(name + '!') == 'A!'`",
                NoneSwept {
                    rows: 2,
                    because: [
                        SelectedOffPath {
                            rows: "TRUE",
                            key: 1,
                        },
                    ],
                },
            ),
        ]
        "#);
    }

    #[tokio::test]
    async fn a_source_without_a_key_column_has_no_row_on_any_path() {
        let info: TableInfo = serde_saphyr::from_str(indoc! {"
            schema: public
            table: places
            srid: 4326
            geometry_column: geom
            geometry_type: POINT
            properties:
              population: int8
            layers:
              places: {}
        "})
        .expect("parses");
        let rows = serde_saphyr::from_str(indoc! {"
            - { population: 5000, geometry: 'POINT(16.37 48.20)' }
        "})
        .expect("parses");
        let memory = Memory {
            tables: vec![MemoryTable {
                table: test_table("places", &info, 14).expect("lowers").expect("has zooms"),
                rows,
            }],
        };
        let review = review(&memory, 14, None, 100, &Reporter::new()).await.expect("reviews");
        let found: Vec<(String, RowSearch)> = review
            .found
            .iter()
            .map(|found| (found.branch.to_string(), found.row.clone()))
            .collect();
        insta::assert_debug_snapshot!(found, @r#"
        [
            (
                "places: every feature",
                NoKey,
            ),
        ]
        "#);
    }

    #[tokio::test]
    async fn the_row_of_the_spec_is_kept_while_it_takes_its_path() {
        let info: TableInfo = serde_saphyr::from_str(indoc! {"
            schema: public
            table: places
            srid: 4326
            geometry_column: geom
            geometry_type: POINT
            id_column: id
            properties:
              id: int8
              population: int8
            layers:
              places:
                where: population > 1000
        "})
        .expect("parses");
        let rows = serde_saphyr::from_str(indoc! {"
            - { id: 1, population: 5000, geometry: 'POINT(16.37 48.20)' }
            - { id: 2, population: 10, geometry: 'POINT(16.38 48.21)' }
            - { id: 3, population: 9000, geometry: 'POINT(16.39 48.22)' }
        "})
        .expect("parses");
        let memory = Memory {
            tables: vec![MemoryTable {
                table: test_table("places", &info, 14).expect("lowers").expect("has zooms"),
                rows,
            }],
        };
        let old: Spec = serde_saphyr::from_str(indoc! {"
            max_zoom: 14
            cases:
              - source: places
                paths: ['places: kept by `where: population > 1000`']
                input: { id: 3 }
                output: []
        "})
        .expect("parses");
        let review = review(&memory, 14, Some(&old), 100, &Reporter::new())
            .await
            .expect("reviews");
        let found: Vec<(String, RowSearch)> = review
            .found
            .iter()
            .map(|found| (found.branch.to_string(), found.row.clone()))
            .collect();
        insta::assert_debug_snapshot!(found, @r#"
        [
            (
                "places: kept by `where: population > 1000`",
                Kept(
                    3,
                ),
            ),
            (
                "places: dropped, `where: population > 1000` is false",
                Swept {
                    key: 2,
                    because: [
                        BackendCannotSelect,
                    ],
                },
            ),
            (
                "places: dropped, `population` is null in `where: population > 1000`",
                NoneSwept {
                    rows: 3,
                    because: [
                        BackendCannotSelect,
                    ],
                },
            ),
        ]
        "#);
    }

    #[tokio::test]
    async fn a_row_of_the_spec_that_left_its_path_is_replaced_by_a_sweep() {
        let info: TableInfo = serde_saphyr::from_str(indoc! {"
            schema: public
            table: places
            srid: 4326
            geometry_column: geom
            geometry_type: POINT
            id_column: id
            properties:
              id: int8
              population: int8
            layers:
              places:
                where: population > 1000
        "})
        .expect("parses");
        let rows = serde_saphyr::from_str(indoc! {"
            - { id: 1, population: 5000, geometry: 'POINT(16.37 48.20)' }
            - { id: 3, population: 10, geometry: 'POINT(16.39 48.22)' }
        "})
        .expect("parses");
        let memory = Memory {
            tables: vec![MemoryTable {
                table: test_table("places", &info, 14).expect("lowers").expect("has zooms"),
                rows,
            }],
        };
        let old: Spec = serde_saphyr::from_str(indoc! {"
            max_zoom: 14
            cases:
              - source: places
                paths: ['places: kept by `where: population > 1000`']
                input: { id: 3 }
                output: []
        "})
        .expect("parses");
        let review = review(&memory, 14, Some(&old), 100, &Reporter::new())
            .await
            .expect("reviews");
        let found: Vec<(String, RowSearch)> = review
            .found
            .iter()
            .map(|found| (found.branch.to_string(), found.row.clone()))
            .collect();
        insta::assert_debug_snapshot!(found, @r#"
        [
            (
                "places: kept by `where: population > 1000`",
                Swept {
                    key: 1,
                    because: [
                        SpecRowOffPath(
                            3,
                        ),
                        BackendCannotSelect,
                    ],
                },
            ),
            (
                "places: dropped, `where: population > 1000` is false",
                Swept {
                    key: 3,
                    because: [
                        BackendCannotSelect,
                    ],
                },
            ),
            (
                "places: dropped, `population` is null in `where: population > 1000`",
                NoneSwept {
                    rows: 2,
                    because: [
                        BackendCannotSelect,
                    ],
                },
            ),
        ]
        "#);
    }

    #[tokio::test]
    async fn a_gone_row_of_the_spec_is_replaced_by_a_sweep() {
        let info: TableInfo = serde_saphyr::from_str(indoc! {"
            schema: public
            table: places
            srid: 4326
            geometry_column: geom
            geometry_type: POINT
            id_column: id
            properties:
              id: int8
              population: int8
            layers:
              places:
                where: population > 1000
        "})
        .expect("parses");
        let rows = serde_saphyr::from_str(indoc! {"
            - { id: 1, population: 5000, geometry: 'POINT(16.37 48.20)' }
        "})
        .expect("parses");
        let memory = Memory {
            tables: vec![MemoryTable {
                table: test_table("places", &info, 14).expect("lowers").expect("has zooms"),
                rows,
            }],
        };
        let old: Spec = serde_saphyr::from_str(indoc! {"
            max_zoom: 14
            cases:
              - source: places
                paths: ['places: kept by `where: population > 1000`']
                input: { id: 9 }
                output: []
        "})
        .expect("parses");
        let review = review(&memory, 14, Some(&old), 100, &Reporter::new())
            .await
            .expect("reviews");
        let found: Vec<(String, RowSearch)> = review
            .found
            .iter()
            .map(|found| (found.branch.to_string(), found.row.clone()))
            .collect();
        insta::assert_debug_snapshot!(found, @r#"
        [
            (
                "places: kept by `where: population > 1000`",
                Swept {
                    key: 1,
                    because: [
                        SpecRowGone(
                            9,
                        ),
                        BackendCannotSelect,
                    ],
                },
            ),
            (
                "places: dropped, `where: population > 1000` is false",
                NoneSwept {
                    rows: 1,
                    because: [
                        BackendCannotSelect,
                    ],
                },
            ),
            (
                "places: dropped, `population` is null in `where: population > 1000`",
                NoneSwept {
                    rows: 1,
                    because: [
                        BackendCannotSelect,
                    ],
                },
            ),
        ]
        "#);
    }

    #[tokio::test]
    async fn generate_writes_a_case_per_row_with_the_paths_it_takes_and_what_bulk_mode_renders() {
        let info: TableInfo = serde_saphyr::from_str(indoc! {"
            schema: public
            table: roads
            srid: 4326
            geometry_column: geom
            geometry_type: LINESTRING
            id_column: osm_id
            properties:
              osm_id: int8
              class: text
              name: text
            layers:
              roads:
                attributes:
                  label: \"name + '!'\"
                rules:
                  - where: \"class == 'motorway'\"
                    minzoom: 4
                  - where: \"class == 'path'\"
                    minzoom: 12
        "})
        .expect("parses");
        let rows = serde_saphyr::from_str(indoc! {"
            - { osm_id: 1, class: motorway, name: A1, geometry: 'LINESTRING(16.37 48.20, 16.40 48.22)' }
            - { osm_id: 2, class: path, geometry: 'LINESTRING(16.37 48.20, 16.38 48.21)' }
            - { osm_id: 3, class: track, name: Feldweg, geometry: 'LINESTRING(16.37 48.20, 16.37001 48.20)' }
        "})
        .expect("parses");
        let memory = Memory {
            tables: vec![MemoryTable {
                table: test_table("roads", &info, 14).expect("lowers").expect("has zooms"),
                rows,
            }],
        };
        let mut review = review(&memory, 14, None, 100, &Reporter::new()).await.expect("reviews");
        review.accept_all();
        insta::assert_snapshot!(yaml(&review.spec()), @r#"
        max_zoom: 14
        cases:
        - source: roads
          paths:
          - "roads: rule 1 `where: class == 'motorway'`"
          - "roads: crosses a z14 tile edge"
          input:
            class: motorway
            geometry: LINESTRING(16.37 48.2,16.4 48.22)
            name: A1
            osm_id: 1
          output:
          - layer: roads
            id: 1
            geometry: line
            zooms: 6-14
            attributes:
              label: A1!
        - source: roads
          paths:
          - "roads: rule 2 `where: class == 'path'`"
          input:
            class: path
            geometry: LINESTRING(16.37 48.2,16.38 48.21)
            name: null
            osm_id: 2
          output:
          - layer: roads
            id: 2
            geometry: line
            zooms: 12-14
        - source: roads
          paths:
          - "roads: no rule matches"
          - "roads: kept only at z14 (`min_size_at_maxzoom`)"
          input:
            class: track
            geometry: LINESTRING(16.37 48.2,16.37001 48.2)
            name: Feldweg
            osm_id: 3
          output:
          - layer: roads
            id: 3
            geometry: line
            zooms: 14-14
            attributes:
              label: Feldweg!
        "#);
    }

    #[tokio::test]
    async fn a_case_of_an_unchanged_row_passes() {
        let info: TableInfo = serde_saphyr::from_str(indoc! {"
            schema: public
            table: places
            srid: 4326
            geometry_column: geom
            geometry_type: POINT
            id_column: id
            properties:
              id: int8
              population: int8
            layers:
              places:
                where: population > 1000
        "})
        .expect("parses");
        let rows = serde_saphyr::from_str(indoc! {"
            - { id: 1, population: 5000, geometry: 'POINT(16.37 48.20)' }
        "})
        .expect("parses");
        let table = test_table("places", &info, 14).expect("lowers").expect("has zooms");
        let memory = Memory {
            tables: vec![MemoryTable {
                table: table.clone(),
                rows,
            }],
        };
        let case: Case = serde_saphyr::from_str(indoc! {"
            source: places
            input: { id: 1, population: 5000, geometry: POINT(16.37 48.2) }
            output:
              - { layer: places, id: 1, geometry: point, zooms: 0-14, attributes: { id: 1, population: 5000 } }
        "})
        .expect("parses");
        let outcome = check(&memory, &[table], &case, 14, false).await.expect("checks");
        insta::assert_snapshot!(shown(&outcome), @"Pass");
    }

    #[tokio::test]
    async fn a_case_whose_row_changed_is_row_changed() {
        let info: TableInfo = serde_saphyr::from_str(indoc! {"
            schema: public
            table: places
            srid: 4326
            geometry_column: geom
            geometry_type: POINT
            id_column: id
            properties:
              id: int8
              population: int8
            layers:
              places:
                where: population > 1000
        "})
        .expect("parses");
        let rows = serde_saphyr::from_str(indoc! {"
            - { id: 1, population: 20000, geometry: 'POINT(16.37 48.20)' }
        "})
        .expect("parses");
        let table = test_table("places", &info, 14).expect("lowers").expect("has zooms");
        let memory = Memory {
            tables: vec![MemoryTable {
                table: table.clone(),
                rows,
            }],
        };
        let case: Case = serde_saphyr::from_str(indoc! {"
            source: places
            input: { id: 1, population: 5000, geometry: POINT(16.37 48.2) }
            output:
              - { layer: places, id: 1, geometry: point, zooms: 0-14, attributes: { id: 1, population: 5000 } }
        "})
        .expect("parses");
        let outcome = check(&memory, &[table], &case, 14, false).await.expect("checks");
        insta::assert_snapshot!(shown(&outcome), @"
        RowChanged
        -input +now
         geometry: POINT(16.37 48.2)
         id: 1
        -population: 5000
        +population: 20000
        -spec +bulk mode now
        @@ - layer: places
           zooms: 0-14
           attributes:
             id: 1
        -    population: 5000
        +    population: 20000
        ");
    }

    #[tokio::test]
    async fn a_case_whose_row_is_gone_is_row_changed() {
        let info: TableInfo = serde_saphyr::from_str(indoc! {"
            schema: public
            table: places
            srid: 4326
            geometry_column: geom
            geometry_type: POINT
            id_column: id
            properties:
              id: int8
              population: int8
            layers:
              places:
                where: population > 1000
        "})
        .expect("parses");
        let rows = serde_saphyr::from_str(indoc! {"
            - { id: 2, population: 5000, geometry: 'POINT(16.37 48.20)' }
        "})
        .expect("parses");
        let table = test_table("places", &info, 14).expect("lowers").expect("has zooms");
        let memory = Memory {
            tables: vec![MemoryTable {
                table: table.clone(),
                rows,
            }],
        };
        let case: Case = serde_saphyr::from_str(indoc! {"
            source: places
            input: { id: 1, population: 5000, geometry: POINT(16.37 48.2) }
            output:
              - { layer: places, id: 1, geometry: point, zooms: 0-14, attributes: { id: 1, population: 5000 } }
        "})
        .expect("parses");
        let outcome = check(&memory, &[table], &case, 14, false).await.expect("checks");
        insta::assert_snapshot!(shown(&outcome), @"
        RowChanged
        -input +now
        -geometry: POINT(16.37 48.2)
        -id: 1
        -population: 5000
        +(the row is gone)
        -spec +bulk mode now
        -- layer: places
        -  id: 1
        -  geometry: point
        -  zooms: 0-14
        -  attributes:
        -    id: 1
        -    population: 5000
        +(no feature in any layer)
        ");
    }

    #[tokio::test]
    async fn a_case_the_layers_render_differently_fails() {
        let info: TableInfo = serde_saphyr::from_str(indoc! {"
            schema: public
            table: places
            srid: 4326
            geometry_column: geom
            geometry_type: POINT
            id_column: id
            properties:
              id: int8
              population: int8
            layers:
              places:
                where: population > 1000
        "})
        .expect("parses");
        let rows = serde_saphyr::from_str(indoc! {"
            - { id: 1, population: 5000, geometry: 'POINT(16.37 48.20)' }
        "})
        .expect("parses");
        let table = test_table("places", &info, 14).expect("lowers").expect("has zooms");
        let memory = Memory {
            tables: vec![MemoryTable {
                table: table.clone(),
                rows,
            }],
        };
        let case: Case = serde_saphyr::from_str(indoc! {"
            source: places
            input: { id: 1, population: 5000, geometry: POINT(16.37 48.2) }
            output:
              - { layer: places, id: 1, geometry: point, zooms: 4-14, attributes: { id: 1, population: 5000 } }
        "})
        .expect("parses");
        let outcome = check(&memory, &[table], &case, 14, false).await.expect("checks");
        insta::assert_snapshot!(shown(&outcome), @"
        Fail
        -spec +bulk mode now
         - layer: places
           id: 1
           geometry: point
        -  zooms: 4-14
        +  zooms: 0-14
           attributes:
             id: 1
             population: 5000
        ");
    }

    #[tokio::test]
    async fn a_case_without_its_key_cannot_run() {
        let info: TableInfo = serde_saphyr::from_str(indoc! {"
            schema: public
            table: places
            srid: 4326
            geometry_column: geom
            geometry_type: POINT
            id_column: id
            properties:
              id: int8
              population: int8
            layers:
              places:
                where: population > 1000
        "})
        .expect("parses");
        let rows = serde_saphyr::from_str(indoc! {"
            - { id: 1, population: 5000, geometry: 'POINT(16.37 48.20)' }
        "})
        .expect("parses");
        let table = test_table("places", &info, 14).expect("lowers").expect("has zooms");
        let memory = Memory {
            tables: vec![MemoryTable {
                table: table.clone(),
                rows,
            }],
        };
        let case: Case = serde_saphyr::from_str(indoc! {"
            source: places
            input: { population: 5000, geometry: POINT(16.37 48.2) }
            output: []
        "})
        .expect("parses");
        let err = check(&memory, &[table], &case, 14, false).await.err().expect("fails");
        insta::assert_snapshot!(err, @"Case of `places` has no id in its input");
    }
}
