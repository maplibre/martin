# Project Overview

Martin is a high-performance tile server serving vector/raster tiles from PostGIS, PMTiles, and MBTiles.
Also serves fonts, sprites, and styles.
Includes `martin-cp` (bulk tile copier) and `mbtiles` (MBTiles CLI) binaries.

## Commands

Uses [just](https://github.com/casey/just) as command runner. `just --list` for all targets

```bash
# build
just check
# run server on :3000
just run
# start test DB (docker) + pmtiles fileserver
just start
# all tests
just test
# all unit tests
cargo test --workspace
# PG tests (needs `just start`)
just test-pg
# integration tests (needs docker)
just test-int
# lint the PR (run before committing)
just lint
# update all snapshot/expected output
just bless
```

## Workspace Structure

4-crate workspace:

- **`martin`** - HTTP server, CLI, configuration, routing. Binaries: `martin`, `martin-cp`
  - `martin-ui/` - React frontend, embedded at build time
- **`martin-core`** - Core source logic `src/tiles/{postgres,mbtiles,pmtiles,cog}/` and `src/resources/{fonts,sprites,styles}/`
  All code that is likely useful for others is here.
- **`martin-tile-utils`** - Tile format detection, compression/decompression
- **`mbtiles`** - MBTiles library + CLI

## Key Rules

- **CI warnings = errors**: `RUSTFLAGS='-D warnings'` in CI. ALL warnings must be fixed.
- **Clippy pedantic** enabled workspace-wide. Avoid `unwrap`/`panic` in non-test code; prefer `expect` with a clear message or proper error handling. `unwrap`/`panic` may be acceptable in tests/examples when appropriate.
- **Integration tests**: compare output against `tests/expected/`. `just bless-int` to update.
- **Frontend-only changes** `martin-ui/` use `npm run dev` to view changes
