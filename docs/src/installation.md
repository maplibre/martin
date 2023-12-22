### Prerequisites

If using Martin with PostgreSQL database, you must install PostGIS with at least v3.0+, v3.1+ recommended.

### Binary Distributions

You can download martin from [GitHub releases page](https://github.com/maplibre/martin/releases).

| Platform | Downloads (latest)     |
|----------|------------------------|
| Linux    | [64-bit][rl-linux-tar] |
| macOS    | [64-bit][rl-macos-tar] |
| Windows  | [64-bit][rl-win64-zip] |

[rl-linux-tar]: https://github.com/maplibre/martin/releases/latest/download/martin-x86_64-unknown-linux-gnu.tar.gz
[rl-macos-tar]: https://github.com/maplibre/martin/releases/latest/download/martin-x86_64-apple-darwin.tar.gz
[rl-win64-zip]: https://github.com/maplibre/martin/releases/latest/download/martin-x86_64-pc-windows-msvc.zip

### Building with Cargo

If you [install Rust](https://www.rust-lang.org/tools/install), you can build martin from source with Cargo:

```shell
cargo install martin
martin --help
```

### Homebrew

If you are using macOS and [Homebrew](https://brew.sh/) you can install martin using Homebrew tap.

```shell
brew tap maplibre/martin
brew install martin
```

### Docker

Martin is also available as a [Docker image](https://ghcr.io/maplibre/martin). You could either share a configuration file from the host with the container via the `-v` param, or you can let Martin auto-discover all sources e.g. by passing `DATABASE_URL` or specifying the .mbtiles/.pmtiles files or URLs to .pmtiles.

```shell
export PGPASSWORD=postgres  # secret!
docker run -p 3000:3000 \
           -e PGPASSWORD \
           -e DATABASE_URL=postgresql://user@host:port/db \
           -v /path/to/config/dir:/config \
           ghcr.io/maplibre/martin --config /config/config.yaml
```
