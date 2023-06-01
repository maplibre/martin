# Using with Docker

You can use official Docker image [`ghcr.io/maplibre/martin`](https://ghcr.io/maplibre/martin)

```shell
docker run \
  -p 3000:3000 \
  -e DATABASE_URL=postgresql://postgres@localhost/db \
  ghcr.io/maplibre/martin
```

If you are running PostgreSQL instance on `localhost`, you have to change network settings to allow the Docker container to access the `localhost` network.

For Linux, add the `--net=host` flag to access the `localhost` PostgreSQL service.

```shell
docker run \
  --net=host \
  -p 3000:3000 \
  -e DATABASE_URL=postgresql://postgres@localhost/db \
  ghcr.io/maplibre/martin
```

For macOS, use `host.docker.internal` as hostname to access the `localhost` PostgreSQL service.

```shell
docker run \
  -p 3000:3000 \
  -e DATABASE_URL=postgresql://postgres@host.docker.internal/db \
  ghcr.io/maplibre/martin
```

For Windows, use `docker.for.win.localhost` as hostname to access the `localhost` PostgreSQL service.

```shell
docker run \
  -p 3000:3000 \
  -e DATABASE_URL=postgresql://postgres@docker.for.win.localhost/db \
  ghcr.io/maplibre/martin
```
