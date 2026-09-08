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

`just py::test` creates a `.venv` in this directory, builds `martin_py` into
it with `maturin develop`, and runs the Python test suite (`tests/`) against
the same fixture files the Rust end-to-end tests use
(`../tests/fixtures/{geojson,pmtiles,duckdb}`). The first build compiles
DuckDB from source (see [Feature scope](#feature-scope) below) and can take
several minutes; later builds reuse Cargo's cache. Run it directly with
pytest once the venv exists:

```bash
just py::develop   # (re)build the extension after a source change
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
`default-features = false, features = ["pmtiles", "geojson"]`, so this crate
serves PMTiles and GeoJSON sources without pulling in `libpq`/DuckDB or the
web UI's npm build. Add `postgres`, `mbtiles`, etc. to that feature list if
bindings for those source kinds are needed too.
