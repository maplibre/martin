#![allow(clippy::unwrap_used)]
use criterion::{BatchSize, Criterion, Throughput, criterion_group, criterion_main};
use mbtiles::{
    BulkTile, CopyDuplicateMode, DedupHint, MbtType, Mbtiles, MbtilesBulkWriter, NormalizedSchema,
    TileCoord, action_with_rusqlite, init_mbtiles_schema,
};
use sqlx::SqliteConnection;
use tempfile::NamedTempFile;
use tokio::runtime::Runtime;

const ZOOM: u8 = 7;
const BATCH: usize = 1000;

const DEDUP_ID: MbtType = MbtType::Normalized {
    hash_view: false,
    schema: NormalizedSchema::DedupId,
};

fn tiles() -> Vec<BulkTile<Vec<u8>>> {
    let mut state = 0x9E37_79B9_7F4A_7C15_u64;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let side = 1_u32 << ZOOM;
    let mut tiles = Vec::new();
    for x in 0..side {
        for y in 0..side {
            let (data, hint) = if next() % 3 == 0 {
                (vec![0xAB; 64], DedupHint::LikelyDuplicate)
            } else {
                let len = 1024 + usize::try_from(next() % 3072).unwrap();
                let data = (0..len).map(|_| next().to_le_bytes()[0]).collect();
                (data, DedupHint::Unique)
            };
            tiles.push(BulkTile {
                coord: TileCoord::new_unchecked(ZOOM, x, y),
                data,
                hint,
            });
        }
    }
    tiles
}

fn new_dst(rt: &Runtime, mbt_type: MbtType) -> (NamedTempFile, Mbtiles, SqliteConnection) {
    let file = NamedTempFile::with_suffix(".mbtiles").unwrap();
    let mbt = Mbtiles::new(file.path()).unwrap();
    let conn = rt.block_on(async {
        let mut conn = mbt.open_or_new().await.unwrap();
        init_mbtiles_schema(&mut conn, mbt_type, false)
            .await
            .unwrap();
        conn
    });
    (file, mbt, conn)
}

fn bench_bulk_insert(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let tiles = tiles();
    let rows: Vec<(u8, u32, u32, Vec<u8>)> = tiles.iter().map(Into::into).collect();

    let mut group = c.benchmark_group("bulk_insert");
    group.sample_size(10);
    group.throughput(Throughput::Elements(u64::try_from(tiles.len()).unwrap()));

    for (name, mbt_type) in [
        ("flat", MbtType::Flat),
        ("flat_with_hash", MbtType::FlatWithHash),
        ("dedup_id", DEDUP_ID),
    ] {
        group.bench_function(format!("insert_tiles/{name}"), |b| {
            b.iter_batched(
                || new_dst(&rt, mbt_type),
                |(_file, mbt, mut conn)| {
                    rt.block_on(async {
                        for batch in rows.chunks(BATCH) {
                            mbt.insert_tiles(&mut conn, mbt_type, CopyDuplicateMode::Abort, batch)
                                .await
                                .unwrap();
                        }
                    });
                },
                BatchSize::PerIteration,
            );
        });

        group.bench_function(format!("bulk_writer/{name}"), |b| {
            b.iter_batched(
                || new_dst(&rt, mbt_type),
                |(_file, _mbt, mut conn)| {
                    rt.block_on(action_with_rusqlite(&mut conn, |c| {
                        let mut writer = MbtilesBulkWriter::new(c, mbt_type)?;
                        for batch in tiles.chunks(BATCH) {
                            writer.write_batch(batch)?;
                        }
                        Ok(())
                    }))
                    .unwrap();
                },
                BatchSize::PerIteration,
            );
        });
    }
    group.finish();
}

criterion_group!(benches, bench_bulk_insert);
criterion_main!(benches);
