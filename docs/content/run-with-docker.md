---
icon: simple/docker
tags:
  - deployment
  - docker
---

# Running with Docker

You can use official Docker image [`ghcr.io/maplibre/martin`](https://ghcr.io/maplibre/martin)

!!! note "Image variants"
    The default image (`:latest`, `:<version>`) is lean and does **not** include server-side style [rendering](sources-styles/rendering.md).
    The batteries-included `-full` variant (`:latest-full`, `:<version>-full`) bundles the `maplibre_native` runtime libraries and enables rendering.
    Use `-full` only if you need rendering; it is a larger image.

### Using Non-Local PostgreSQL

```bash
docker run \
  -p 3000:3000 \
  ghcr.io/maplibre/martin:2.0.0 \
  postgres://postgres@postgres.example.org/db
```

### Exposing Local Files

You can expose local files to the Docker container using the `-v` flag.

```bash
docker run \
  -p 3000:3000 \
  -v /path/to/local/files:/files \
  ghcr.io/maplibre/martin:2.0.0 \
  /files
```

You can also pass any [CLI flags](run-with-cli.md) after the image name, for example `--webui enable-for-all` to serve the built-in web UI to all clients (by default it is only served to connections from localhost, which inside a container excludes the host):

```bash
docker run \
  -p 3000:3000 \
  -v /path/to/local/files:/files \
  ghcr.io/maplibre/martin:2.0.0 \
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
  ghcr.io/maplibre/martin:2.0.0 \
  postgres://postgres@localhost/db
```

### Accessing Local PostgreSQL on macOS

For macOS, use `host.docker.internal` as hostname to access the `localhost` PostgreSQL service.

```bash
docker run \
  -p 3000:3000 \
  ghcr.io/maplibre/martin:2.0.0 \
  postgres://postgres@host.docker.internal/db
```

### Accessing Local PostgreSQL on Windows

For Windows, use `docker.for.win.localhost` as hostname to access the `localhost` PostgreSQL service.

```bash
docker run \
  -p 3000:3000 \
  ghcr.io/maplibre/martin:2.0.0 \
  postgres://postgres@docker.for.win.localhost/db
```
