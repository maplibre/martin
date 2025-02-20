# Development

Clone Martin, setting remote name to `upstream`. This way `main` branch will be updated automatically with the latest
changes from the upstream repo.

```bash, ignore
git clone https://github.com/maplibre/martin.git -o upstream
cd martin
```

Fork Martin repo into your own GitHub account, and add your fork as a remote

```bash, ignore
git remote add origin  _URL_OF_YOUR_FORK_
```

Install [docker](https://docs.docker.com/get-docker/) and [docker-compose](https://docs.docker.com/compose/)

```bash, ignore
# Ubuntu-based distros have an older version that might also work:
sudo apt install -y  docker.io docker-compose
```

Install a few required libs and tools:

```bash, ignore
# For Ubuntu-based distros
sudo apt install -y  build-essential pkg-config jq file gdal-bin
```

Install [Just](https://github.com/casey/just#readme) (improved makefile processor). Note that some Linux and Homebrew
distros have outdated versions of Just, so you should install it from source:

```bash, ignore
cargo install just --locked
```

When developing MBTiles SQL code, you may need to use `just prepare-sqlite` whenever SQL queries are modified.
Run `just` to see all available commands.
