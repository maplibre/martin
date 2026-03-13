# Agents

## Cursor Cloud specific instructions

### Project overview

Martin is a high-performance map tile server written in Rust. See `README.md` for project overview and `.github/copilot-instructions.md` for detailed contributor directives (scope isolation, timeouts, validation protocols). The `justfile` documents all available dev commands (`just --list`).

### Services

| Service | How to start | Port | Notes |
|---|---|---|---|
| PostGIS test database | `just start` | 5411 | Required for PG tests and full integration tests |
| NGINX fileserver (PMTiles) | `just start-pmtiles-server` | 5412 | Required for integration tests only |
| Martin tile server | `just run` | 3000 | Serves tiles from configured sources |

### Docker daemon

The cloud VM runs inside a nested container. Docker is configured with `fuse-overlayfs` storage driver and `iptables-legacy`. The daemon must be started manually before any `docker compose` commands:

```bash
sudo dockerd &>/tmp/dockerd.log &
sleep 3
sudo chmod 666 /var/run/docker.sock
```

### libstdc++ linker issue

The build requires a `libstdc++.so` symlink that may be missing. If you see `cannot find -lstdc++` during `cargo build`, create the symlink:

```bash
sudo ln -sf /usr/lib/gcc/x86_64-linux-gnu/13/libstdc++.so /usr/lib/x86_64-linux-gnu/libstdc++.so
```

### Lint / Test / Build / Run

Standard commands are documented in `justfile` and `docs/content/development/index.md`. Key commands:

- **Lint**: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets`, `just biomejs-martin-ui`, `just type-check`
- **Test (no PG)**: `cargo test --all-targets` (runs unit tests without requiring PostgreSQL)
- **Test (with PG)**: `just test-pg` (starts DB + runs PG-specific tests)
- **Test (frontend)**: `cd martin/martin-ui && npm run test -- --run`
- **Build**: `cargo build --workspace`
- **Run**: `just run` (starts Martin with `--webui enable-for-all`)

### Environment variables

The `justfile` sets key defaults automatically: `DATABASE_URL=postgres://postgres:postgres@localhost:5411/db`, `AWS_SKIP_CREDENTIALS=1`, `AWS_REGION=eu-central-1`. No manual env setup needed when using `just` commands.

### Frontend (martin-ui)

Located at `martin/martin-ui`. Install dependencies with `npm ci --no-fund`. Frontend tests use Vitest and run via `npm run test -- --run`. The frontend is embedded into the Martin binary at compile time via `build.rs`.
