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

`maturin develop` builds the wheel in a temporary directory, installs it into
the active venv (with `uv run`, that's this crate's venv) and then deletes it.
To get a wheel file that another project can depend on, use `maturin build`.

### Building a wheel with a stable name

```bash
uv run maturin build --release --compatibility linux
# -> ../target/wheels/martin_py-0.1.0-cp312-cp312-linux_x86_64.whl
```

Always pass `--compatibility linux`. Without it, maturin checks which glibc
symbol versions the built `.so` uses and tags the wheel with the matching
manylinux policy, e.g. `manylinux_2_39_x86_64` on a glibc 2.39 host. That tag
changes with the build machine's glibc, so the wheel's filename changes too.
Projects that pin the wheel by path then break. In fastgeoapi, that's the
`[tool.uv.sources]` entry in `pyproject.toml` and the matching `uv.lock` entry.
The plain `linux_x86_64` tag stays the same from one build to the next.
It works for local path installs, but PyPI won't accept it.

After rebuilding, reinstall the wheel in the project that consumes it. The
filename doesn't change, but the contents and hash do:

```bash
cd ../../fastgeoapi
uv lock --upgrade-package martin-py
uv sync --reinstall-package martin-py
```

For a portable wheel, e.g. one for PyPI or for Docker images based on older
distros, target an explicit, lower manylinux policy instead. For example,
`--compatibility manylinux_2_28 --zig` (needs `pip install ziglang`), or build
inside the `ghcr.io/pyo3/maturin` container.

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
