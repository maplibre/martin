# Generation Architecture

> [!WARNING]
>
> **Not released or fully implemented**
>
> The bulk mode of `martin cp` is not in any release yet, and parts of what these pages describe are not implemented yet. It is only built with the `unstable-generate` feature:
>
> ```bash
> cargo build --package martin --bin martin --features unstable-generate
> ```
>
> Not implemented yet:
>
> - [ ] Rendering points, lines and polygons, with fill ranges for covered tiles
> - [ ] Tile grouping, MLT and MVT encoding, compression and deduplication
> - [ ] Planning tables into layers and running a whole generation end to end
> - [ ] The PostgreSQL source with partitioned whole-table scans
> - [ ] Bulk mode in `martin cp`, and PMTiles output
> - [ ] Layers with CEL expressions, `rules` and `sort_by`
> - [ ] OSM PBF, shapefile, GeoPackage and CSV files as sources
> - [ ] `geometry` other than `point`, `line` and `polygon`
> - [ ] `tile` operations (`merge_lines`, `merge_polygons`, `merge_multi`, `label_grid`, `dedup_by`, `limit`)

The [bulk mode](<https://maplibre.org/martin/martin-generate/index.md>) of `martin cp` turns whole sources into a tileset in two phases. First, every feature is read once and rendered into tile pieces, which are sorted on disk by tile. Then the sorted pieces are merged back into tiles, encoded, and written in the order the archive stores them. Every step hands its work to the next through a bounded queue, so the whole run needs a fixed amount of memory, whatever the input size.

We implement this via two traits:

- a `FeatureSource` streams features from partitions that can be read in parallel, and
- a `TileSink` writes finished tiles in the order it asks for.

Another database or file format is another `FeatureSource`, and another archive format is another `TileSink`.

The tile server renders a table source with `layers` with the same engine, one tile or one block of tiles at a time and in memory, see [Serving tiles](<#serving-tiles>).

## Motivation and Planetiler

A long-requested Martin feature is generating tiles on the fly from a feature source (raw data) inside Martin itself, when serving tiles. Until now, every source had to either already contain tiles (MBTiles, PMTiles) or generate them itself, like PostgreSQL does with `ST_AsMVT`. Martin only passed those tiles along.

Martin adapted the tile generation method of [Planetiler](<https://github.com/onthegomap/planetiler>), written by Mike Barry ([@msbarry](<https://github.com/msbarry>)) and licensed under the [Apache License 2.0](<https://github.com/onthegomap/planetiler/blob/main/LICENSE>). Thank you, Mike, for an amazing tool, and thanks to the Planetiler contributors. Martin also adapted a similar layer configuration system, which takes features from any source Martin supports that can produce a stream of features (a `FeatureSource`).

As part of this effort, Martin can now generate a whole tileset at once, as Planetiler does. This expands the existing [`martin cp`](<https://maplibre.org/martin/martin-cp/index.md>) command: only its internals change, so that it supports more than one method of copying data from a source to an archive. Besides asking the source for each tile, it can read each source's features once and build the tiles itself. The tile server uses the same engine to render the tiles of a PostgreSQL table source with `layers` as they are requested.

This method follows Planetiler in:

- reading every feature once and rendering it at every zoom, instead of querying the source per tile,
- sorting the rendered pieces on disk by tile, then merging them back and writing each tile once,
- covering the inside of large polygons with [fill ranges](<#rendering>) instead of one piece per tile,
- simplification and minimum size [in pixels of a 256-pixel tile](<https://maplibre.org/martin/sources-pg-layers/#simplification-and-minimum-size>), with the same defaults,
- [encoding tile batches in parallel and writing them in order](<#parallel-encoding-ordered-writing>),
- reusing the previous tile's bytes and hashing only likely duplicates, see [repeated tiles](<#repeated-tiles>),
- storing repeated tiles once, in the same deduplicated MBTiles layout.

## Phase 1: read and render

### Partitions

A `FeatureSource` declares how many partitions it has, and streams one partition when asked. Readers read different partitions at the same time, at most one reader per thread. In return, a source promises two things:

- **Stable order**: every partition is read in the same row order on every run. With the partition numbers, that fixes the order of the features whatever the thread timing, see [the sort key](<#sort-key>). It need not be the order of one scan, so a source can split its data however it reads fastest.
- **Consistent data**: every partition is read from the same state of the data, even if it changes during the run.

How a source splits its data is up to the source. A file can split by byte range or row group, a database by page range or by id.

The PostgreSQL source splits each table by what the relation allows:

| Relation | Partitions |
| --- | --- |
| Tables and materialized views | Page ranges (`ctid >= '(start,0)' AND ctid < '(end,0)'`), 16384 pages each. The last range is open, so rows added since are still read |
| Anything else with an integer `id_column` | 32 partitions by id remainder (`abs(id % 32) = k`). The first also takes `NULL` ids |
| Anything else | One scan |

Partitions are numbered table by table in physical order. For consistent data, one connection exports a snapshot (`pg_export_snapshot()`), and every reader imports it in a `REPEATABLE READ` transaction before reading its first partition. PostgreSQL's own parallel workers and synchronized scans are turned off on these connections, so each partition is read in its own order, at the pace the generator asks for. There is one reader per two render workers, and no more readers than the pool's `pool_size`.

Each scan reads only the rows some layer takes, when the database can apply every layer's `geometry` and `where`, see [Features](<https://maplibre.org/martin/sources-pg-layers/#features>).

### Feature queue

Readers claim the next partition from a shared counter and stream it as batches of up to 1024 features. The batches go into one queue that holds at most `threads × 2` batches, and every render worker takes the next batch from it. When the workers fall behind, the queue fills and the readers wait, which in turn stops reading from the source. Memory does not grow with the source, and fast readers never race ahead of slow rendering.

Because any worker can take any batch, a slow partition or a feature-heavy area never stalls the other workers.

### Rendering

A render worker takes each feature through every layer its source feeds:

1. The layer's filters decide whether the feature belongs to the layer, and its rules which settings apply.
2. The feature is projected to Web Mercator once, then rendered from its highest zoom down. At each zoom a line or polygon is simplified, and dropped once it is smaller than the minimum size, which also ends the lower zooms, since a feature only shrinks with the zoom. Points are rendered at every zoom.
3. The geometry is sliced into the tiles it touches, keeping its own vertices. No new vertices are cut at tile edges. As in tile mode, a tile keeps what lies on the edge of its buffer too, except a clipped line or polygon that only touches it from outside. A polygon ring that leaves a tile is closed by corners just outside the tile's buffer, where renderers clip it away. A feature that keeps clear of every neighbor tile's buffer, as most do at most zooms, skips the slicer entirely. With `clip_geom: false`, every tile a line or polygon reaches gets the whole geometry instead.
4. Each piece is stored as a record. Its vertices are stored as offsets from the previous vertex, which usually fit in one or two bytes.

The tiles a polygon covers entirely become **fill ranges** instead of one record per tile. A fill range is one record that stands for a contiguous run of tile ids in the archive's tile order, which is a column of tiles in MBTiles order or an aligned quadtree cell in Hilbert order. A huge polygon such as an ocean or a country then costs time and space in proportion to its outline, not its area. An unclipped polygon gets no fill ranges: each covered tile stores it whole.

### Sort key

Every record is sorted by a 16-byte key:

Sorting by this key puts the pieces of each tile next to each other, in the order the archive writes its tiles. Inside a tile, it puts the layers in their configured order, and inside a layer, it keeps the features in source order. The `Seq` part is the feature's partition and row, which do not depend on how the partitions were spread over threads. The tiles are therefore the same whatever the number of threads and their timing.

A layer's `sort_by` value is not part of the key. It travels in the record, and the encoder orders each layer's features by it, keeping the `Seq` order of equal values.

Without `sort_by`, a layer keeps source order, which depends on how the source splits its data. The PostgreSQL source splits a view by id remainder, for example, which interleaves its ids:

Only `sort_by` gives a layer an order that does not depend on the partitions.

### External sort

Each render worker owns a sort buffer of `--memory / threads`, at least 16 MiB and at most 4 GiB. Records are appended to the buffer as they are rendered, and an index of 24-byte entries (key, offset, length) is kept beside them. The budget counts the index twice, because the radix sort needs a second copy of it while sorting. When the records and the index reach the budget, the worker spills the buffer:

1. It radix-sorts the index by key, without moving the records. If the records arrived in increasing `Seq`, as with a single reader, sorting by tile and layer is enough, since the radix sort is stable.
2. It writes the records in key order to a new temporary file, a **sorted run**, as key, length and record, repeated.
3. It clears the buffer and goes on rendering.

Runs go round-robin to the `--temp-dir` directories. They are anonymous temporary files, which the operating system removes when the process ends, even when it is killed.

When every worker has spilled its last buffer, phase 1 ends.

## Phase 2: merge, encode and write

### Merge

A loser-tree merge reads up to 256 runs at once, each through a 256 KiB buffer, and streams their records in key order into phase 2. Only the pieces of one feature in one tile can have equal keys. They come from one worker, and keep the order it pushed them in.

With more than 256 runs, only the excess is merged first: adjacent runs, just enough that merging them in groups of 256 leaves 256 runs in all, picked where they hold the fewest bytes. The groups are merged in parallel into new temp files. The other runs are left for the final merge, which is never written to disk. One such pass is enough for up to 65 536 runs. Merging only adjacent runs keeps equal keys in the order they were pushed.

### Grouping

The grouper collects the merged records of one tile at a time. A fill range is kept aside while it still covers upcoming tiles, and added to each of them, so a fill range costs one record however many tiles it covers. The grouper hands tiles on in batches of 256. A batch closes early once it holds 16 MiB of records. The few huge tiles of low zooms then go to several encoders.

### Parallel encoding, ordered writing

Encoding is the expensive part of phase 2, so it runs on all threads, but the archive must receive the tiles in order. The ordered pipeline does both, with a bounded number of batches in flight:

- For each batch, the grouper first puts an empty result **slot** into the **order queue**, then hands the batch to the work queue. The order queue holds `threads × 4` slots. When it is full, the grouper waits.
- Encoders take batches from the work queue in any order, finish them in any order, and put each result into its batch's slot.
- The writer takes slots from the order queue in batch order and waits for each slot's result. A result that arrives early waits in its slot until the writer gets to it.

When one batch is slow, for example a dense city at a high zoom, the other encoders go on with later batches until the order queue is full, and then wait. Memory stays bounded however slow one batch is, and the archive still sees every tile in order.

### Repeated tiles

Each encoder assembles the tile's layers from its records, encodes them as MLT or MVT, and compresses the result. It also avoids work and storage for repeated tiles:

- A tile with the same records as the encoder's previous tile reuses its bytes without encoding. Neighboring tiles in the archive order are often identical, for example the inside of a large polygon.
- A reused tile, and a tile made only of fill records, gets a dedup key from an index shared by all encoders. The index only hands out a key for bytes equal to the first tile with that hash.

The archive stores tiles with the same dedup key once. Other identical tiles are stored separately.

### Tile order

The archive decides the tile order, and the whole pipeline produces tiles in that order, so the writer never sorts or buffers:

| Archive | Tile order | Why |
| --- | --- | --- |
| MBTiles | Zoom, column, then bottom-up TMS row | The primary key of the `tiles` table, so inserts append to its B-tree instead of splitting pages. Always the deduplicated (`normalized`) schema |
| PMTiles | Hilbert tile id | A clustered archive, written as a stream. Tiles with a dedup key share their bytes |

The tileset's metadata, including the zooms, the format and each layer's fields, is only complete after the last tile, so the archive stores it last.

## Serving tiles

The tile server renders a table source with `layers` with the plan, renderer, grouper and encoder of bulk mode, without its queues and temp files. [Serving](<https://maplibre.org/martin/sources-pg-layers/#serving>) covers its configuration. A request renders a **block**: the tile alone, or with `prefetch`, the block of tiles holding it, see [Prefetch](<#prefetch>).

1. **Read**: one query reads every feature whose geometry meets the block grown by the largest `buffer` of the table's layers. When the grown block crosses the antimeridian, the part past it is read as a second envelope, as bulk mode wraps buffers there too. The query returns whole geometries in the order PostgreSQL finds them, and the features are numbered in that order, as one partition. The query keeps only rows some layer at the block's zoom takes, so `max_feature_count` limits how many of those it returns. Only with `max_feature_count`, `order_by` sorts them by `id_column` first, to choose which are kept.
2. **Render**: one render worker, on a blocking thread, renders the features at the block's zoom only, and slices them into its tiles only, never into the other tiles they reach. The minimum size and simplification still follow the layer's zooms, so each tile comes out as bulk mode writes it for the same `maxzoom`.
3. **Sort**: the sort buffer stays in memory. It grows as records arrive instead of reserving its budget, and is never written to a temp file. The merge reads its records back in key order.
4. **Group and encode**: the grouper and encoder of phase 2, in MBTiles order, write an uncompressed MVT tile for each tile of the block. A tile no feature reaches is empty.

The server then converts, compresses and caches each tile, as it does for any other source.

Inside a layer, the features keep the order the query returns them in, so a layer without `sort_by` can draw them in another order than bulk mode, see [Draw order](<https://maplibre.org/martin/sources-pg-layers/#draw-order>).

### Prefetch

With `prefetch: N`, a cache miss renders the N×N block holding the tile, in one query, and caches all of its tiles, empty ones included. Blocks are aligned to multiples of N, so a request for any tile of a block renders that same block, and is a cache hit once it is rendered. A block at the edge of the world only holds the tiles that exist, so `prefetch: 4` at zoom 1 renders the 2×2 tiles of that zoom.

Each tile goes through the same processing as a requested one, and is cached under the key the request would have for it, for the same source and accepted format. Without a tile cache, or at a zoom outside the cache's zooms, a request renders its tile alone, since nothing could keep the others.

## Memory and disk

Every queue and buffer has a bound that does not depend on the size of the input:

| What | Bound |
| --- | --- |
| Feature queue | `threads × 2` batches of up to 1024 features |
| Sort buffers | `--memory` in total, records plus index, at least 16 MiB per worker |
| Run file writers | 1 MiB of write buffer for each run being written |
| Merge | 256 KiB of read buffer per run, at most 256 runs, so 64 MiB per merge |
| Encode window | `threads × 4` batches of 256 tiles or 16 MiB, plus the one being written |
| Dedup index | One copy of each distinct tile that got a dedup key |

An intermediate merge, with more than 256 runs, runs several 64 MiB merges at once. On disk, a feature has a record in every tile it touches, at every zoom, and an intermediate merge needs room for another copy of the runs it merges, which are the fewest bytes it can merge.

[Serving a block](<#serving-tiles>) has no such bound and uses no disk: it holds every feature reaching the block, and their records, in memory until its tiles are encoded. A larger `prefetch` holds more.

## Errors

- In phase 1, an error stops every reader from claiming another partition and every worker from taking another batch. The run reports that error, not the hand-offs it interrupted.
- In phase 2, an error of the grouper or an encoder travels to the writer in batch order, as an error in place of a batch. The archive stops there and never finishes a partial tileset. When the writer fails, the grouper's next hand-off fails, and the writer's error is reported.
- Expressions never fail a run, see [Expressions](<https://maplibre.org/martin/sources-pg-layers/#expressions>). Features the slicer cannot handle at a zoom are skipped at that zoom and counted.
- A failed or interrupted (<kbd>Ctrl</kbd>+<kbd>C</kbd>) run removes the output file. The sorted runs are removed by the operating system in every case.
- A served block logs the features the slicer could not handle as warnings. Expression failures are only logged at debug level there, since the plan already warns about the first failure of each expression. A failed query or encoding fails the request.
