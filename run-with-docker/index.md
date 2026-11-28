# Running with Docker

The official image is [`ghcr.io/maplibre/martin`](<https://ghcr.io/maplibre/martin>). Its entrypoint is `martin`, so everything after the image name in `docker run` is passed to Martin as [CLI arguments](<https://maplibre.org/martin/run-with-cli/index.md>).

## Image tags

| Tag | Contents | Platforms |
| --- | --- | --- |
| `:<version>`, `:latest` | Martin without server-side style [rendering](<https://maplibre.org/martin/sources-styles/rendering/index.md>) | `linux/amd64`, `linux/arm64` |
| `:<version>-full`, `:latest-full` | Martin with rendering, plus the EGL and Mesa libraries it needs | `linux/amd64`, `linux/arm64` |
| `:<version>-legacycpu`, `:latest-legacycpu` | Static Martin for x86\_64 CPUs without AVX2, without rendering | `linux/amd64` |

Use `-full` only if you need rendering, because it is a larger image.

Every stable release also moves a major and a minor tag, such as `:2` and `:2.0`, with the variant suffix if there is one, such as `:2-full`. Use them to follow every release of one major or minor version. Prereleases, such as `2.0.0-beta.8`, publish only their own `:<version>` tags.

## Exposing Local Files

Mount the directory that holds your files with `-v` and pass its path inside the container to Martin:

```bash
docker run \
  -p 3000:3000 \
  -v /path/to/local/files:/files \
  ghcr.io/maplibre/martin:2.0.0-beta.8 \
  /files
```

Martin is now available at [localhost:3000](<http://localhost:3000/>).

Pass any other [CLI flags](<https://maplibre.org/martin/run-with-cli/index.md>) after the image name. For example, `--webui enable-for-all` serves the built-in web UI to all clients. By default, Martin serves the web UI only to connections from localhost, which inside a container excludes the host.

```bash
docker run \
  -p 3000:3000 \
  -v /path/to/local/files:/files \
  ghcr.io/maplibre/martin:2.0.0-beta.8 \
  --webui enable-for-all \
  /files
```

> [!NOTE]
>
> **File permissions**
>
> Martin runs as the `nonroot` user, uid and gid `65532`. The files that Martin serves must be readable by that user, or by the user you set with `--user`.

## Using Non-Local PostgreSQL

Pass the connection string after the image name:

```bash
docker run \
  -p 3000:3000 \
  ghcr.io/maplibre/martin:2.0.0-beta.8 \
  postgres://postgres@postgres.example.org/db
```

## Accessing Local PostgreSQL on Linux

Inside a container, `localhost` is the container itself, not your machine. On Linux, add `--net=host` so the container shares the host network and reaches a PostgreSQL server on `localhost`. You do not need `-p`, because Martin listens on the host network directly.

```bash
docker run \
  --net=host \
  ghcr.io/maplibre/martin:2.0.0-beta.8 \
  postgres://postgres@localhost/db
```

## Accessing Local PostgreSQL on macOS

On macOS, use `host.docker.internal` as the hostname of a PostgreSQL server on `localhost`:

```bash
docker run \
  -p 3000:3000 \
  ghcr.io/maplibre/martin:2.0.0-beta.8 \
  postgres://postgres@host.docker.internal/db
```

## Accessing Local PostgreSQL on Windows

On Windows, use `docker.for.win.localhost` as the hostname of a PostgreSQL server on `localhost`:

```bash
docker run \
  -p 3000:3000 \
  ghcr.io/maplibre/martin:2.0.0-beta.8 \
  postgres://postgres@docker.for.win.localhost/db
```

## Writing Files to a Bind Mount

Martin runs as uid `65532`, so it can only write to directories that this uid can write to. This affects the output of `martin cp` and `--save-config`. To write files that you own into a bind mount, run the container as your own user with `--user`:

```bash
docker run \
  --user "$(id -u):$(id -g)" \
  -v "$PWD:/data" \
  ghcr.io/maplibre/martin:2.0.0-beta.8 \
  cp --source world --max-zoom 10 --output-file /data/world.mbtiles /data/world.pmtiles
```

This example copies zoom levels 0 to 10 of `./world.pmtiles` into `./world.mbtiles`. Without `--user`, it fails with a permission error unless uid `65532` can write to the current directory.

## Health Check

The image's `HEALTHCHECK` runs `martin health`, which needs no shell.

| Behavior | Value |
| --- | --- |
| Request | One `GET` to `http://127.0.0.1:3000/health`, ignoring `HTTP_PROXY` and similar variables |
| Change URL | `--url <url>` or the `MARTIN_HEALTH_URL` environment variable |
| Exit code `0` | The server answered with a `2xx` status |
| Exit code `1` | Any other status, a connection error, or no answer within 5 seconds. The reason is printed to stderr. |

Martin answers `/health` only after all sources have loaded. While sources load, each check times out after 5 seconds and fails.

If Martin listens on a port other than `3000`, set `MARTIN_HEALTH_URL` to the health URL on that port:

```bash
docker run \
  -p 8080:8080 \
  -e MARTIN_HEALTH_URL=http://127.0.0.1:8080/health \
  -v /path/to/local/files:/files \
  ghcr.io/maplibre/martin:2.0.0-beta.8 \
  --listen-addresses 0.0.0.0:8080 \
  /files
```

To see the result of the checks, read the container's health status. It is `starting`, `healthy` or `unhealthy`:

```bash
docker inspect --format '{{.State.Health.Status}}' <container>
```

> [!TIP]
>
> **Slow startup**
>
> If loading your sources takes longer than about a minute, Docker can mark the container `unhealthy` before Martin is ready. Add `--health-start-period 5m`, or the time your sources need, so that failed checks during startup do not count. The `--health-interval`, `--health-timeout`, `--health-retries` and `--health-start-period` flags keep the image's check command.

> [!WARNING]
>
> **Do not use `--health-cmd`**
>
> Docker runs a `--health-cmd` command through `/bin/sh`, which the images do not have. Every check then fails, and Docker marks the container `unhealthy`. Use `MARTIN_HEALTH_URL` to change what the image's check requests.

## Image Contents

The images contain Martin and the [Wolfi](<https://wolfi.dev>) packages Martin loads, on an empty `scratch` base. They have no shell and no package manager, and run as the `nonroot` user, uid and gid `65532`.

`docker exec` still works for commands that are binaries in the image. For example, to run a health check by hand:

```bash
docker exec <container> /usr/local/bin/martin health
```

`docker exec <container> sh` fails because the image has no shell. To inspect a running container with a shell, use `docker debug <container>`, which needs Docker Desktop.

> [!NOTE]
>
> **Derived images**
>
> A Dockerfile that starts `FROM ghcr.io/maplibre/martin` cannot `RUN` commands. To add tools, copy the Martin binary into a base image of your own. See the [migration guide](<https://maplibre.org/martin/migration-guide/index.md#docker-images-have-no-shell-and-run-as-a-non-root-user>) for the libraries each variant needs.
