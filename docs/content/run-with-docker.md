---
icon: simple/docker
tags:
  - deployment
  - docker
---

# Running with Docker

You can use official Docker image [`ghcr.io/maplibre/martin`](https://ghcr.io/maplibre/martin)

### Using Non-Local PostgreSQL

Pass the connection string as an argument. Martin does not read `DATABASE_URL` on its own -- see
[environment variables](env-vars.md) if you would rather keep it in a variable and reference it
from a config file.

```bash
docker run \
  -p 3000:3000 \
  ghcr.io/maplibre/martin:1.14.0 \
  postgres://postgres@postgres.example.org/db
```

### Exposing Local Files

You can expose local files to the Docker container using the `-v` flag.

```bash
docker run \
  -p 3000:3000 \
  -v /path/to/local/files:/files \
  ghcr.io/maplibre/martin:1.14.0 \
  /files
```

You can also pass any [CLI flags](run-with-cli.md) after the image name, for example `--webui enable-for-all` to serve the built-in web UI (disabled by default):

```bash
docker run \
  -p 3000:3000 \
  -v /path/to/local/files:/files \
  ghcr.io/maplibre/martin:1.14.0 \
  --webui enable-for-all \
  /files
```

### Accessing Local PostgreSQL on Linux

If you are running PostgreSQL instance on `localhost`, you have to change network settings to allow the Docker container
to access the `localhost` network.

For Linux, add the `--net=host` flag to access the `localhost` PostgreSQL service.
You would not need to export ports with `-p` because the container is already using the host network.

```bash
docker run \
  --net=host \
  ghcr.io/maplibre/martin:1.14.0 \
  postgres://postgres@localhost/db
```

### Accessing Local PostgreSQL on macOS

For macOS, use `host.docker.internal` as hostname to access the `localhost` PostgreSQL service.

```bash
docker run \
  -p 3000:3000 \
  ghcr.io/maplibre/martin:1.14.0 \
  postgres://postgres@host.docker.internal/db
```

### Accessing Local PostgreSQL on Windows

For Windows, use `docker.for.win.localhost` as hostname to access the `localhost` PostgreSQL service.

```bash
docker run \
  -p 3000:3000 \
  ghcr.io/maplibre/martin:1.14.0 \
  postgres://postgres@docker.for.win.localhost/db
```
