### Prerequisites

If using Martin with PostgreSQL database, you must install PostGIS with at least v3.0+. Postgis v3.1+ is recommended.

### Docker

Martin is also available as a [Docker image](https://ghcr.io/maplibre/martin). You could either share a configuration
file from the host with the container via the `-v` param, or you can let Martin auto-discover all sources e.g. by
passing `DATABASE_URL` or specifying the .mbtiles/.pmtiles files or URLs to .pmtiles.

```bash
export PGPASSWORD=postgres  # secret!

docker run -p 3000:3000 \
           -e PGPASSWORD \
           -e DATABASE_URL=postgresql://user@host:port/db \
           -v /path/to/config/dir:/config \
           ghcr.io/maplibre/martin --config /config/config.yaml
```

### From Binary Distributions Manually

You can download martin from [GitHub releases page](https://github.com/maplibre/martin/releases).

| Platform | x64                                                                                              | ARM-64                              |
|----------|--------------------------------------------------------------------------------------------------|-------------------------------------|
| Linux    | [.tar.gz][rl-linux-x64] (gnu)<br>[.tar.gz][rl-linux-x64-musl] (musl)<br>[.deb][rl-linux-x64-deb] | [.tar.gz][rl-linux-a64-musl] (musl) |
| macOS    | [.tar.gz][rl-macos-x64]                                                                          | [.tar.gz][rl-macos-a64]             |
| Windows  | [.zip][rl-win64-zip]                                                                             |                                     |

[rl-linux-x64]: https://github.com/maplibre/martin/releases/latest/download/martin-x86_64-unknown-linux-gnu.tar.gz

[rl-linux-x64-musl]: https://github.com/maplibre/martin/releases/latest/download/martin-x86_64-unknown-linux-musl.tar.gz

[rl-linux-x64-deb]: https://github.com/maplibre/martin/releases/latest/download/martin-Debian-x86_64.deb

[rl-linux-a64-musl]: https://github.com/maplibre/martin/releases/latest/download/martin-aarch64-unknown-linux-musl.tar.gz

[rl-macos-x64]: https://github.com/maplibre/martin/releases/latest/download/martin-x86_64-apple-darwin.tar.gz

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

To install with apt source and others, We need your help
to [improve packaging for various platforms](https://github.com/maplibre/martin/issues/578).

#### Homebrew

If you are using macOS and [Homebrew](https://brew.sh/) you can install martin using Homebrew tap.

```bash
brew tap maplibre/martin
brew install martin
martin --help
```

#### Debian Packages(x86_64) manually

```bash
curl -O https://github.com/maplibre/martin/releases/latest/download/martin-Debian-x86_64.deb
sudo dpkg -i ./martin-Debian-x86_64.deb
martin --help
rm ./martin-Debian-x86_64.deb
```

### Building From source

If you [install Rust](https://www.rust-lang.org/tools/install), you can build martin from source with Cargo:

```bash
cargo install martin --locked
martin --help
```
