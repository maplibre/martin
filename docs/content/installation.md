---
icon: material/download
tags:
  - getting-started
  - deployment
---

### Prerequisites

If using Martin with PostgreSQL database, you must install PostGIS with at least v3.0+. PostGIS v3.1+ is recommended.

### Docker

Martin is also available as a [Docker image](https://ghcr.io/maplibre/martin). You could either share a configuration
file from the host with the container via the `-v` param, or you can let Martin auto-discover all sources e.g. by
passing `DATABASE_URL` or specifying the .mbtiles/.pmtiles files or URLs to .pmtiles.

```bash
export PGPASSWORD=postgres  # secret!

docker run -p 3000:3000 \
           -e PGPASSWORD \
           -e DATABASE_URL=postgres://user@host:port/db \
           -v /path/to/config/dir:/config \
           ghcr.io/maplibre/martin:1.16.0 \
           --config /config/config.yaml
```

### From Binary Distributions Manually

You can download martin from [GitHub releases page](https://github.com/maplibre/martin/releases).

| Platform | x64                                                                                              | ARM-64                                                                   |
|----------|--------------------------------------------------------------------------------------------------|--------------------------------------------------------------------------|
| Linux    | [.tar.gz][rl-linux-x64] (gnu)<br>[.tar.gz][rl-linux-x64-musl] (musl)<br>[.deb][rl-linux-x64-deb] | [.tar.gz][rl-linux-a64-gnu] (gnu)<br>[.tar.gz][rl-linux-a64-musl] (musl) |
| macOS    |                                                                                                  | [.tar.gz][rl-macos-a64]                                                  |
| Windows  | [.zip][rl-win64-zip]                                                                             |                                                                          |

[rl-linux-x64]: https://github.com/maplibre/martin/releases/latest/download/martin-x86_64-unknown-linux-gnu.tar.gz

[rl-linux-x64-musl]: https://github.com/maplibre/martin/releases/latest/download/martin-x86_64-unknown-linux-musl.tar.gz

[rl-linux-x64-deb]: https://github.com/maplibre/martin/releases/latest/download/debian-x86_64.deb

[rl-linux-a64-gnu]: https://github.com/maplibre/martin/releases/latest/download/martin-aarch64-unknown-linux-gnu.tar.gz

[rl-linux-a64-musl]: https://github.com/maplibre/martin/releases/latest/download/martin-aarch64-unknown-linux-musl.tar.gz

[rl-macos-a64]: https://github.com/maplibre/martin/releases/latest/download/martin-aarch64-apple-darwin.tar.gz

[rl-win64-zip]: https://github.com/maplibre/martin/releases/latest/download/martin-x86_64-pc-windows-msvc.zip

Rust users can install pre-built martin binary
with [cargo-binstall](https://github.com/cargo-bins/cargo-binstall) and `cargo`.

```bash
cargo install cargo-binstall
cargo binstall martin
martin --help
```

### From package

To install with apt source and others, we need your help
to [improve packaging for various platforms](https://github.com/maplibre/martin/issues/578).

#### Homebrew

If you are using [Homebrew](https://brew.sh/) you can install martin using

```bash
brew install martin
martin --help
```

#### Debian packages (x86_64) manually

```bash
curl -O https://github.com/maplibre/martin/releases/latest/download/debian-x86_64.deb
sudo dpkg -i ./debian-x86_64.deb
martin --help
rm ./debian-x86_64.deb
```

#### Arch Linux

The [AUR](https://aur.archlinux.org/packages/martin) carries `martin` and `martin-cp`, maintained by the community.
With an AUR helper such as `yay`:

```bash
yay -S martin
martin --help
```

#### Nix

[nixpkgs](https://search.nixos.org/packages?query=martin) carries `martin`, usually a release or two behind.

```bash
nix-shell -p martin --run 'martin --help'
```

### Building from source

If you [install Rust](https://www.rust-lang.org/tools/install), you can build martin from source with Cargo:

```bash
cargo install martin --locked
martin --help
```

#### Optional features

Features prefixed with `unstable-` are **not included** in default builds, Homebrew, Debian packages, or the Docker image.
To experiment with them, build Martin from source with the feature enabled:

```bash
cargo install martin --locked --features=unstable-duckdb
```

The currently available unstable features are `unstable-cog` for [COG sources](sources-cog-files.md)
and `unstable-duckdb` for [DuckDB / GeoParquet sources](sources-duckdb.md).

#### Platform-Specific Build Notes

##### Windows

When building from source on Windows, please note the following feature limitations:

- **`rendering`**: This feature is **not available on Windows**. It requires `maplibre_native` which currently only supports MacOS and Linux. For updates, see [`maplibre/maplibre-native-rs`](https://github.com/maplibre/maplibre-native-rs).
