# martin-py

`PyO3` bindings exposing Martin's **tile endpoints** to Python, in-process and
without going through HTTP. It mirrors what the `martin` server's
`/{source_ids}` and `/{source_ids}/{z}/{x}/{y}` routes do, but calls the same
source-resolution and tile-fetching code directly against a `TileServer`
loaded from an ordinary Martin config file.

Other endpoints (fonts, sprites, styles, rendering) are out of scope for this
crate; see [`martin::srv`](../martin/src/srv/mod.rs) if you need those too.

## Building

```bash
pip install maturin
maturin develop --release   # builds martin_py and installs it into the active venv
```

## Testing

The commands below use [`just`](https://github.com/casey/just), the command
runner this repo uses for all `just <recipe>` tasks. Install it with one of:

```bash
cargo install just   # from source, via Cargo (any platform)
cargo binstall just   # same, but fetches a prebuilt binary (faster)
brew install just     # macOS/Linux via Homebrew
# or the official install script (installs to ./bin by default):
curl --proto '=https' --tlsv1.2 -sSf https://just.systems/install.sh | bash -s -- --to ./bin
```

See the [`just` installation docs](https://just.systems/man/en/packages.html)
for more package-manager options.

`just test` creates a `.venv` in this directory, builds `martin_py` into
it with `maturin develop`, and runs the Python test suite (`tests/`) against
the same fixture files the Rust end-to-end tests use
(`../tests/fixtures/{geojson,pmtiles,duckdb}`). The first build compiles
DuckDB from source (see [Feature scope](#feature-scope) below) and can take
several minutes; later builds reuse Cargo's cache. Run it directly with
pytest once the venv exists:

```bash
just develop   # (re)build the extension after a source change
.venv/bin/pytest
```

## Usage

```python
import martin_py

server = martin_py.TileServer("martin-config.yaml")
print(server.list_sources())

tilejson = server.get_source_info("my_source")
data, content_type, content_encoding = server.get_tile("my_source", 0, 0, 0)
```

`get_tile` returns the tile's raw bytes exactly as they would be sent over
HTTP (so still compressed, when the source produces compressed tiles) plus
its `Content-Type` and, if present, `Content-Encoding`.

## Feature scope

The `martin` dependency in [Cargo.toml](Cargo.toml) is built with
`default-features = false, features = ["pmtiles", "geojson", "unstable-duckdb"]`,
so this crate serves PMTiles, GeoJSON, and DuckDB/GeoParquet sources without
pulling in `libpq` or the web UI's npm build. `unstable-duckdb` compiles
DuckDB from source on the first build (no system install needed, but it's
slow - see [Testing](#testing) above). Add `postgres`, `mbtiles`, etc. to that
feature list if bindings for those source kinds are needed too.
