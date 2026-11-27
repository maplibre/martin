---
description: Running the official Martin Docker image with PostgreSQL or files
icon: simple/docker
tags:
  - deployment
  - docker
---

# Running with Docker

You can use official Docker image [`ghcr.io/maplibre/martin`](https://ghcr.io/maplibre/martin)

!!! note "Image variants"
    The default image (`:latest`, `:<version>`) is lean and does **not** include server-side style [rendering](sources-styles/rendering.md).
    The batteries-included `-full` variant (`:latest-full`, `:<version>-full`) bundles the EGL and Mesa libraries rendering needs and enables it.
    Use `-full` only if you need rendering; it is a larger image.
    The `-legacycpu` variant runs on x86_64 CPUs without AVX2.
    `:2`, `:2-full` and `:2-legacycpu` (and `:2.0`, ...) follow the newest release of that major or minor version.

The images hold only Martin and the [Wolfi](https://wolfi.dev) packages it loads: there is no shell or package manager.
They run as the `nonroot` user (uid and gid `65532`), so a directory Martin writes to, such as the output of `martin cp`, must be writable by that user.
Run as yourself with `--user "$(id -u):$(id -g)"` to write into your own directories.

### Health check

The images check their health with `martin health`, which asks `http://127.0.0.1:3000/health` and exits `0` on success.
If Martin listens on another port, set `MARTIN_HEALTH_URL` to the URL to ask instead:

```bash
docker run \
  -p 8080:8080 \
  -e MARTIN_HEALTH_URL=http://127.0.0.1:8080/health \
  -v /path/to/local/files:/files \
  ghcr.io/maplibre/martin:2.0.0-beta.8 \
  --listen-addresses 0.0.0.0:8080 \
  /files
```

`docker run --health-cmd` always runs its command through a shell, which the images do not have.

### Using Non-Local PostgreSQL

```bash
docker run \
  -p 3000:3000 \
  ghcr.io/maplibre/martin:2.0.0-beta.8 \
  postgres://postgres@postgres.example.org/db
```

### Exposing Local Files

You can expose local files to the Docker container using the `-v` flag.

```bash
docker run \
  -p 3000:3000 \
  -v /path/to/local/files:/files \
  ghcr.io/maplibre/martin:2.0.0-beta.8 \
  /files
```

You can also pass any [CLI flags](run-with-cli.md) after the image name, for example `--webui enable-for-all` to serve the built-in web UI to all clients (by default it is only served to connections from localhost, which inside a container excludes the host):

```bash
docker run \
  -p 3000:3000 \
  -v /path/to/local/files:/files \
  ghcr.io/maplibre/martin:2.0.0-beta.8 \
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
  ghcr.io/maplibre/martin:2.0.0-beta.8 \
  postgres://postgres@localhost/db
```

### Accessing Local PostgreSQL on macOS

For macOS, use `host.docker.internal` as hostname to access the `localhost` PostgreSQL service.

```bash
docker run \
  -p 3000:3000 \
  ghcr.io/maplibre/martin:2.0.0-beta.8 \
  postgres://postgres@host.docker.internal/db
```

### Accessing Local PostgreSQL on Windows

For Windows, use `docker.for.win.localhost` as hostname to access the `localhost` PostgreSQL service.

```bash
docker run \
  -p 3000:3000 \
  ghcr.io/maplibre/martin:2.0.0-beta.8 \
  postgres://postgres@docker.for.win.localhost/db
```
