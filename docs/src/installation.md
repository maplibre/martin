## Prerequisites

If using Martin with PostgreSQL database, you must install PostGIS with at least v3.0+, v3.1+ recommended.

## Binary Distributions

You can download martin from [GitHub releases page](https://github.com/maplibre/martin/releases).

| Platform | Downloads (latest)     |
|----------|------------------------|
| Linux    | [64-bit][rl-linux-tar] |
| macOS    | [64-bit][rl-macos-tar] |
| Windows  | [64-bit][rl-win64-zip] |

[rl-linux-tar]: https://github.com/maplibre/martin/releases/latest/download/martin-Linux-x86_64.tar.gz
[rl-macos-tar]: https://github.com/maplibre/martin/releases/latest/download/martin-Darwin-x86_64.tar.gz
[rl-win64-zip]: https://github.com/maplibre/martin/releases/latest/download/martin-Windows-x86_64.zip

# Building with Cargo

If you [install Rust](https://www.rust-lang.org/tools/install), you can build martin from source with Cargo:

```shell
cargo install martin
martin --help
```

If your PostgreSQL connection requires SSL, you would need to install OpenSSL and run `cargo install martin --features ssl`, or even install with `--features vendored-openssl` to [statically link OpenSSL](https://docs.rs/openssl/latest/openssl/#vendored) into the binary.

## Homebrew

If you are using macOS and [Homebrew](https://brew.sh/) you can install martin using Homebrew tap.

```shell
brew tap maplibre/martin https://github.com/maplibre/martin.git
brew install maplibre/martin/martin
```

## Docker

Martin is also available as a [Docker image](https://ghcr.io/maplibre/martin). You could either share a configuration file from the host with the container via the `-v` param, or you can let Martin auto-discover all sources e.g. by passing `DATABASE_URL` or specifying the .mbtiles/.pmtiles files.

```shell
export PGPASSWORD=postgres  # secret!
docker run -p 3000:3000 \
           -e PGPASSWORD \
           -e DATABASE_URL=postgresql://user@host:port/db \
           -v /path/to/config/dir:/config \
           ghcr.io/maplibre/martin --config /config/config.yaml
```
