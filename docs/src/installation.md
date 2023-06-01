# Installation

## Prerequisites

Martin requires PostGIS 3.0+.  PostGIS 3.1+ is recommended.

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

If you are using macOS and [Homebrew](https://brew.sh/) you can install martin using Homebrew tap.

```shell
brew tap maplibre/martin https://github.com/maplibre/martin.git
brew install maplibre/martin/martin
```

## Docker

You can also use [official Docker image](https://ghcr.io/maplibre/martin)

```shell
export PGPASSWORD=postgres  # secret!
docker run \
       -p 3000:3000 \
       -e PGPASSWORD \
       -e DATABASE_URL=postgresql://postgres@localhost/db \
       ghcr.io/maplibre/martin
```

Use docker `-v` param to share configuration file or its directory with the container:

```shell
export PGPASSWORD=postgres  # secret!
docker run -v /path/to/config/dir:/config \
           -p 3000:3000 \
           -e PGPASSWORD \
           -e DATABASE_URL=postgresql://postgres@localhost/db \
           ghcr.io/maplibre/martin --config /config/config.yaml
```
