use std::io::{Read as _, Write as _};
use std::path::PathBuf;

use bytes::Bytes;
use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use mbtiles::{MbtType, Mbtiles, MbtilesTranscoder, NormalizedSchema};
use sqlx::SqliteConnection;
use tempfile::NamedTempFile;

const NORM_WITH_VIEW: MbtType = MbtType::Normalized {
    hash_view: true,
    schema: NormalizedSchema::Hash,
};

fn gzip_roundtrip(data: Vec<u8>) -> Result<Bytes, Box<dyn std::error::Error + Send + Sync>> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::fast());
    encoder.write_all(&data)?;
    let compressed = encoder.finish()?;
    let mut decoder = GzDecoder::new(&compressed[..]);
    let mut decompressed = Vec::new();
    decoder.read_to_end(&mut decompressed)?;
    Ok(Bytes::from(decompressed))
}

async fn setup_source(name: &str, script: &str) -> (Mbtiles, SqliteConnection, PathBuf) {
    mbtiles::temp_named_mbtiles(name, script).await
}

fn new_dst() -> NamedTempFile {
    NamedTempFile::with_suffix("mbtiles").unwrap()
}

fn bench_transcode(c: &mut Criterion) {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("failed to build tokio runtime");

    let flat_script = include_str!("../../tests/fixtures/mbtiles/world_cities.sql");
    let norm_script = include_str!("../../tests/fixtures/mbtiles/geography-class-png.sql");

    let (_flat_mbt, _flat_conn, flat_src) =
        rt.block_on(setup_source("bench_flat_src", flat_script));
    let (_norm_mbt, _norm_conn, norm_src) =
        rt.block_on(setup_source("bench_norm_src", norm_script));

    let mut group = c.benchmark_group("transcode");

    let src = flat_src.clone();
    group.bench_function("flat_to_flat", |b| {
        b.to_async(&rt).iter_batched(
            new_dst,
            |dst| {
                let src = src.clone();
                async move {
                    MbtilesTranscoder::new(src, dst.path().to_path_buf(), |data| {
                        Ok(Bytes::from(data))
                    })
                    .dst_type(MbtType::Flat)
                    .run()
                    .await
                    .unwrap();
                }
            },
            BatchSize::PerIteration,
        );
    });

    let src = flat_src.clone();
    group.bench_function("flat_to_normalized", |b| {
        b.to_async(&rt).iter_batched(
            new_dst,
            |dst| {
                let src = src.clone();
                async move {
                    MbtilesTranscoder::new(src, dst.path().to_path_buf(), |data| {
                        Ok(Bytes::from(data))
                    })
                    .dst_type(NORM_WITH_VIEW)
                    .run()
                    .await
                    .unwrap();
                }
            },
            BatchSize::PerIteration,
        );
    });

    let src = norm_src.clone();
    group.bench_function("normalized_to_normalized", |b| {
        b.to_async(&rt).iter_batched(
            new_dst,
            |dst| {
                let src = src.clone();
                async move {
                    MbtilesTranscoder::new(src, dst.path().to_path_buf(), |data| {
                        Ok(Bytes::from(data))
                    })
                    .dst_type(NORM_WITH_VIEW)
                    .run()
                    .await
                    .unwrap();
                }
            },
            BatchSize::PerIteration,
        );
    });

    let src = norm_src.clone();
    group.bench_function("normalized_to_flat", |b| {
        b.to_async(&rt).iter_batched(
            new_dst,
            |dst| {
                let src = src.clone();
                async move {
                    MbtilesTranscoder::new(src, dst.path().to_path_buf(), |data| {
                        Ok(Bytes::from(data))
                    })
                    .dst_type(MbtType::Flat)
                    .run()
                    .await
                    .unwrap();
                }
            },
            BatchSize::PerIteration,
        );
    });

    let src = flat_src.clone();
    group.bench_function("flat_to_flat_with_hash", |b| {
        b.to_async(&rt).iter_batched(
            new_dst,
            |dst| {
                let src = src.clone();
                async move {
                    MbtilesTranscoder::new(src, dst.path().to_path_buf(), |data| {
                        Ok(Bytes::from(data))
                    })
                    .dst_type(MbtType::FlatWithHash)
                    .run()
                    .await
                    .unwrap();
                }
            },
            BatchSize::PerIteration,
        );
    });

    let src = norm_src.clone();
    group.bench_function("normalized_to_flat_with_hash", |b| {
        b.to_async(&rt).iter_batched(
            new_dst,
            |dst| {
                let src = src.clone();
                async move {
                    MbtilesTranscoder::new(src, dst.path().to_path_buf(), |data| {
                        Ok(Bytes::from(data))
                    })
                    .dst_type(MbtType::FlatWithHash)
                    .run()
                    .await
                    .unwrap();
                }
            },
            BatchSize::PerIteration,
        );
    });

    let src = flat_src.clone();
    group.bench_function("flat_to_flat_gzip_roundtrip", |b| {
        b.to_async(&rt).iter_batched(
            new_dst,
            |dst| {
                let src = src.clone();
                async move {
                    MbtilesTranscoder::new(src, dst.path().to_path_buf(), gzip_roundtrip)
                        .dst_type(MbtType::Flat)
                        .run()
                        .await
                        .unwrap();
                }
            },
            BatchSize::PerIteration,
        );
    });

    group.finish();
}

criterion_group!(benches, bench_transcode);
criterion_main!(benches);
