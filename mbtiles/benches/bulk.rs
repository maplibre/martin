//! Every way to write tiles into every schema, into a new file on disk. Prints a markdown table of the
//! results, as shown in the README, after criterion's own report.

use std::cell::RefCell;
use std::fmt::Write as _;
use std::time::{Duration, Instant};

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use mbtiles::{
    CopyDuplicateMode, MbtType, Mbtiles, NormalizedSchema, TileCoord, TileDedup,
    init_mbtiles_schema,
};
use sqlx::{SqliteConnection, query};
use tempfile::TempDir;

const TILES: u32 = 20_000;
const TILE_BYTES: usize = 200;
/// What `martin cp` hands to `insert_tiles` at a time.
const CP_BATCH: usize = 1000;
const BAR_WIDTH: f64 = 24.0;

const SCHEMAS: [(&str, MbtType); 5] = [
    ("flat", MbtType::Flat),
    ("flat-with-hash", MbtType::FlatWithHash),
    (
        "normalized",
        MbtType::Normalized {
            hash_view: false,
            schema: NormalizedSchema::Hash,
        },
    ),
    (
        "dedup-id",
        MbtType::Normalized {
            hash_view: false,
            schema: NormalizedSchema::DedupId,
        },
    ),
    ("cache", MbtType::Cache),
];

#[derive(Clone, Copy)]
enum Method {
    InsertTiles,
    InsertTilesNoSync,
    BulkUnknown,
    BulkHinted,
}

/// Criterion id, README label, method.
const METHODS: [(&str, &str, Method); 4] = [
    ("insert_tiles", "`insert_tiles`", Method::InsertTiles),
    (
        "insert_tiles-nosync",
        "`insert_tiles`, `synchronous=OFF`",
        Method::InsertTilesNoSync,
    ),
    (
        "bulk-unknown",
        "`bulk_write`, `Unknown`",
        Method::BulkUnknown,
    ),
    (
        "bulk-hinted",
        "`bulk_write`, `Key`/`Unique`",
        Method::BulkHinted,
    ),
];

struct Tile {
    coord: TileCoord,
    data: Vec<u8>,
    /// What a generator knows: the fill tiles share a key, the rest are unique.
    hint: TileDedup,
}

/// Zoom 8 tiles; every fourth is the same fill tile, as polygon interiors make them.
fn tiles() -> Vec<Tile> {
    (0..TILES)
        .map(|i| {
            let coord = TileCoord::new_unchecked(8, i % 256, i / 256);
            if i % 4 == 0 {
                Tile {
                    coord,
                    data: vec![7; TILE_BYTES],
                    hint: TileDedup::Key(1),
                }
            } else {
                let data = i.to_le_bytes().into_iter().cycle().take(TILE_BYTES);
                Tile {
                    coord,
                    data: data.collect(),
                    hint: TileDedup::Unique,
                }
            }
        })
        .collect()
}

async fn new_file(mbt_type: MbtType) -> (TempDir, Mbtiles, SqliteConnection) {
    let dir = tempfile::tempdir().expect("temp dir");
    let mbt = Mbtiles::new(dir.path().join("bench.mbtiles")).expect("mbtiles path");
    let mut conn = mbt.open_or_new().await.expect("open");
    init_mbtiles_schema(&mut conn, mbt_type, false)
        .await
        .expect("schema");
    (dir, mbt, conn)
}

async fn write(
    mbt: &Mbtiles,
    conn: &mut SqliteConnection,
    mbt_type: MbtType,
    method: Method,
    tiles: &[Tile],
) {
    let mode = CopyDuplicateMode::Abort;
    match method {
        Method::InsertTiles | Method::InsertTilesNoSync => {
            if matches!(method, Method::InsertTilesNoSync) {
                let sql = query!("PRAGMA synchronous = OFF");
                sql.execute(&mut *conn).await.expect("pragma");
            }
            for chunk in tiles.chunks(CP_BATCH) {
                let batch: Vec<_> = chunk
                    .iter()
                    .map(|t| (t.coord.z(), t.coord.x(), t.coord.y(), t.data.as_slice()))
                    .collect();
                mbt.insert_tiles(conn, mbt_type, mode, &batch)
                    .await
                    .expect("insert_tiles");
            }
        }
        Method::BulkUnknown | Method::BulkHinted => {
            mbt.bulk_write(conn, mbt_type, mode, |writer| {
                for t in tiles {
                    let hinted = matches!(method, Method::BulkHinted);
                    let dedup = if hinted { t.hint } else { TileDedup::Unknown };
                    writer.write(t.coord, &t.data, dedup)?;
                }
                Ok::<_, mbtiles::MbtError>(())
            })
            .await
            .expect("bulk write");
        }
    }
}

fn bench_writes(c: &mut Criterion) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    let tiles = tiles();
    // Seconds per run of every (schema, method), warm-up runs included: they do the same work.
    let runs = RefCell::new(vec![Vec::new(); SCHEMAS.len() * METHODS.len()]);
    let mut group = c.benchmark_group("write");
    group
        .sample_size(10)
        .throughput(Throughput::Elements(u64::from(TILES)));
    for (s, &(schema, mbt_type)) in SCHEMAS.iter().enumerate() {
        for (m, &(id, _, method)) in METHODS.iter().enumerate() {
            group.bench_function(format!("{schema}/{id}"), |b| {
                b.iter_custom(|iters| {
                    let mut total = Duration::ZERO;
                    for _ in 0..iters {
                        let (_dir, mbt, mut conn) = rt.block_on(new_file(mbt_type));
                        let start = Instant::now();
                        rt.block_on(write(&mbt, &mut conn, mbt_type, method, &tiles));
                        let elapsed = start.elapsed();
                        runs.borrow_mut()[s * METHODS.len() + m].push(elapsed.as_secs_f64());
                        total += elapsed;
                    }
                    total
                });
            });
        }
    }
    group.finish();
    #[expect(clippy::print_stdout, reason = "the table is what this bench reports")]
    {
        println!("{}", table(&runs.into_inner()));
    }
}

/// Median run time per case, in `SCHEMAS` × `METHODS` order.
fn medians(runs: &[Vec<f64>]) -> Vec<f64> {
    runs.iter()
        .map(|r| {
            let mut r = r.clone();
            r.sort_by(f64::total_cmp);
            r[r.len() / 2]
        })
        .collect()
}

fn table(runs: &[Vec<f64>]) -> String {
    let secs = medians(runs);
    let fastest = secs.iter().copied().fold(f64::INFINITY, f64::min);
    let mut out = format!(
        "\n{TILES} tiles of {TILE_BYTES} bytes, a quarter of them identical; median of all runs.\n\n\
         | Schema | Method | Time | Tiles/s | vs `insert_tiles` | Throughput |\n\
         |---|---|--:|--:|--:|:--|\n"
    );
    for (s, (schema, _)) in SCHEMAS.iter().enumerate() {
        let baseline = secs[s * METHODS.len()];
        for (m, (_, method, _)) in METHODS.iter().enumerate() {
            let t = secs[s * METHODS.len() + m];
            let name = if m == 0 {
                format!("**{schema}**")
            } else {
                String::new()
            };
            writeln!(
                out,
                "| {name} | {method} | {} | {} | {:.1}× | `{}` |",
                duration(t),
                per_second(f64::from(TILES) / t),
                baseline / t,
                bar(fastest / t),
            )
            .expect("write to string");
        }
    }
    out
}

fn duration(secs: f64) -> String {
    if secs >= 1.0 {
        format!("{secs:.2} s")
    } else {
        format!("{:.0} ms", secs * 1e3)
    }
}

fn per_second(rate: f64) -> String {
    if rate >= 1e6 {
        format!("{:.2} M", rate / 1e6)
    } else {
        format!("{:.0} k", rate / 1e3)
    }
}

/// A bar `fraction` of the full width, in eighths of a character.
fn bar(fraction: f64) -> String {
    const EIGHTHS: [&str; 8] = ["", "▏", "▎", "▍", "▌", "▋", "▊", "▉"];
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a small non-negative width"
    )]
    let eighths = ((fraction * BAR_WIDTH * 8.0).round() as usize).max(1);
    format!("{}{}", "█".repeat(eighths / 8), EIGHTHS[eighths % 8])
}

criterion_group!(benches, bench_writes);
criterion_main!(benches);
