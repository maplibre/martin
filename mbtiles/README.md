# mbtiles

[![Book](https://img.shields.io/badge/docs-Book-informational)](https://maplibre.org/martin/tools.html)
[![docs.rs docs](https://docs.rs/mbtiles/badge.svg)](https://docs.rs/mbtiles)
[![](https://img.shields.io/badge/Slack-%23maplibre--martin-blueviolet?logo=slack)](https://slack.openstreetmap.us/)
[![GitHub](https://img.shields.io/badge/github-maplibre/martin-8da0cb?logo=github)](https://github.com/maplibre/martin)
[![crates.io version](https://img.shields.io/crates/v/mbtiles.svg)](https://crates.io/crates/mbtiles)
[![CI build](https://github.com/maplibre/martin/actions/workflows/ci.yml/badge.svg)](https://github.com/maplibre/martin/actions)

A library to help tile servers like [Martin](https://maplibre.org/martin) work with [MBTiles](https://github.com/mapbox/mbtiles-spec) files.
When using as a lib, you may want to disable default features (i.e. the unused "cli" feature).

This crate also has a small utility that allows users to interact with the `*.mbtiles` files from the command line.  See [tools](https://maplibre.org/martin/tools.html) documentation for more information.

## Writing tiles

There are two ways to write tiles, and both work with every schema:

* [`Mbtiles::insert_tiles`] is async and writes one batch per transaction. Each tile is a separate statement, and each statement goes through sqlx's worker thread. It suits small and incremental writes.
* [`Mbtiles::bulk_write`] hands a closure a [`MbtilesBulkWriter`], which writes synchronously on the connection's raw `rusqlite` handle. It reuses its prepared statements, commits every 65,536 tiles (its `batch_size`), and turns off `synchronous` until it's done. It suits generating whole files. A [`TileDedup`] hint per tile tells it what it needs to hash:
  * `Unknown` compares bytes by their hash, like `insert_tiles`;
  * `Unique` skips the hash;
  * `Key(k)` marks tiles the caller already knows to be identical, so they're hashed and stored once. A key reused for different bytes fails the write.

  In the dedup-id schema, `Unknown` tiles use the connection's hash index of stored blobs, which `insert_tiles` uses too. Once that index is in use, hinted tiles go through it as well, so it never misses a blob.

The bulk writer is exclusive by design:
* The closure never sees the raw connection, and the connection can't be used for anything else until the closure returns.
* The writer can only be created by `bulk_write`. The closure borrows it, so it can't be leaked or kept past the call.
* Returning `Ok` commits, and an error or a panic rolls back.
* It takes the file's write lock up front, so other connections wait.

Two things are left to the caller:
* A failed write keeps the tiles of earlier periodic commits. Set `writer.batch_size = None` to make the write all or nothing; the journal then grows with the data until the single commit.
* The closure runs on the calling thread, so call `bulk_write` from a blocking context, not from an async worker thread.

```rust,no_run
use mbtiles::{CopyDuplicateMode, MbtError, MbtType, Mbtiles, TileCoord, TileDedup};

# async fn example(mbt: &Mbtiles, conn: &mut sqlx::SqliteConnection) -> Result<(), MbtError> {
mbt.bulk_write(conn, MbtType::Flat, CopyDuplicateMode::Abort, |writer| {
    writer.write(TileCoord::new_unchecked(0, 0, 0), b"tile bytes", TileDedup::Unique)?;
    Ok::<_, MbtError>(())
})
.await
# }
```

### Performance

The table below shows 20,000 tiles of 200 bytes, a quarter of them identical, written to a new file. Each row is the median of all runs on an Intel i9-10885H with an SSD (ext4). `insert_tiles` gets batches of 1,000 tiles, as in `martin cp`. The bar shows throughput relative to the fastest case. Normalized files with a hash view write at the same speed as those without one.

| Schema | Method | Time | Tiles/s | vs `insert_tiles` | Throughput |
|---|---|--:|--:|--:|:--|
| **flat** | `insert_tiles` | 1.05 s | 19 k | 1.0× | `███▏` |
|  | `insert_tiles`, `synchronous=OFF` | 714 ms | 28 k | 1.5× | `████▌` |
|  | `bulk_write`, `Unknown` | 135 ms | 148 k | 7.7× | `████████████████████████` |
|  | `bulk_write`, `Key`/`Unique` | 147 ms | 136 k | 7.1× | `██████████████████████` |
| **flat-with-hash** | `insert_tiles` | 1.31 s | 15 k | 1.0× | `██▌` |
|  | `insert_tiles`, `synchronous=OFF` | 963 ms | 21 k | 1.4× | `███▍` |
|  | `bulk_write`, `Unknown` | 203 ms | 99 k | 6.4× | `████████████████` |
|  | `bulk_write`, `Key`/`Unique` | 183 ms | 109 k | 7.1× | `█████████████████▊` |
| **normalized** | `insert_tiles` | 1.93 s | 10 k | 1.0× | `█▋` |
|  | `insert_tiles`, `synchronous=OFF` | 1.47 s | 14 k | 1.3× | `██▎` |
|  | `bulk_write`, `Unknown` | 292 ms | 68 k | 6.6× | `███████████▏` |
|  | `bulk_write`, `Key`/`Unique` | 275 ms | 73 k | 7.0× | `███████████▉` |
| **dedup-id** | `insert_tiles` | 2.70 s | 7 k | 1.0× | `█▎` |
|  | `insert_tiles`, `synchronous=OFF` | 2.32 s | 9 k | 1.2× | `█▍` |
|  | `bulk_write`, `Unknown` | 347 ms | 58 k | 7.8× | `█████████▍` |
|  | `bulk_write`, `Key`/`Unique` | 136 ms | 147 k | 19.8× | `███████████████████████▉` |
| **cache** | `insert_tiles` | 1.56 s | 13 k | 1.0× | `██▏` |
|  | `insert_tiles`, `synchronous=OFF` | 1.10 s | 18 k | 1.4× | `███` |
|  | `bulk_write`, `Unknown` | 390 ms | 51 k | 4.0× | `████████▍` |
|  | `bulk_write`, `Key`/`Unique` | 392 ms | 51 k | 4.0× | `████████▎` |

Most of `insert_tiles`' time goes to sending each statement to sqlx's worker thread, not to syncing the disk: turning `synchronous` off gains only a little. Hints matter most for the dedup-id schema. With `Unknown`, it must hash every tile and look the hash up; with `Key` and `Unique`, it hashes nothing. Running `cargo bench -p mbtiles --bench bulk` prints this table.

[`Mbtiles::insert_tiles`]: https://docs.rs/mbtiles/latest/mbtiles/struct.Mbtiles.html#method.insert_tiles
[`Mbtiles::bulk_write`]: https://docs.rs/mbtiles/latest/mbtiles/struct.Mbtiles.html#method.bulk_write
[`MbtilesBulkWriter`]: https://docs.rs/mbtiles/latest/mbtiles/struct.MbtilesBulkWriter.html
[`TileDedup`]: https://docs.rs/mbtiles/latest/mbtiles/enum.TileDedup.html

### Development

Any changes to SQL commands require running of `just prepare-sqlite`.  This will install `cargo sqlx` command if it is not already installed, and update the `./sqlx-data.json` file.

## License

Licensed under either of

* Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
* MIT license ([LICENSE-MIT](LICENSE-MIT) or <http://opensource.org/licenses/MIT>)
  at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in the work by you, as defined in the
Apache-2.0 license, shall be dual licensed as above, without any
additional terms or conditions.
