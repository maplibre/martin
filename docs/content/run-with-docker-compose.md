---
description: Running Martin and PostGIS together with Docker Compose
icon: simple/docker
tags:
  - deployment
  - docker
  - docker-compose
---

# Running with Docker Compose

You can use example [`docker-compose.yml`](https://raw.githubusercontent.com/maplibre/martin/main/docker-compose.yml)
file as a reference

```yml
services:
  martin:
    image: ghcr.io/maplibre/martin:2.0.0-beta.8
    restart: unless-stopped
    ports:
      - "3000:3000"
    command:
      - postgres://postgres:password@db/db
    depends_on:
      - db

  db:
    image: postgis/postgis:17-3.5-alpine
    restart: unless-stopped
    environment:
      - POSTGRES_DB=db
      - POSTGRES_USER=postgres
      - POSTGRES_PASSWORD=password
    volumes:
      # persist PostgreSQL data in a local directory outside of the docker container
      - ./pg_data:/var/lib/postgresql/data
```

First, you need to start `db` service

```bash
docker compose up -d db
```

Then, after `db` service is ready to accept connections, you can start `martin`

```bash
docker compose up -d martin
```

By default, Martin will be available at [localhost:3000](http://localhost:3000/)

## Health check

The official image has a `HEALTHCHECK` that runs `martin health`, and Docker Compose uses it without extra configuration.
See [Health Check](run-with-docker.md#health-check) for what `martin health` checks.

If Martin listens on a port other than `3000`, set `MARTIN_HEALTH_URL` to the health URL on that port:

```yml
services:
  martin:
    image: ghcr.io/maplibre/martin:2.0.0-beta.8
    ports:
      - "8080:8080"
    command:
      - --listen-addresses=0.0.0.0:8080
      - postgres://postgres:password@db/db
    environment:
      - MARTIN_HEALTH_URL=http://127.0.0.1:8080/health
```

To change only the timing, set `interval`, `timeout`, `retries` or `start_period` and leave out `test`.
Docker then keeps the image's check command.
If loading your sources takes longer than about a minute, raise `start_period` so that Docker does not mark Martin `unhealthy` while it starts:

```yml
services:
  martin:
    healthcheck:
      start_period: 5m
```

!!! warning "Use the exec form in `test`"
    The image has no shell.
    A `test` in the shell form, such as `CMD-SHELL` or a plain string, fails on every check.
    A `test` of your own must use the exec form, `["CMD", ...]`:

    ```yml
    services:
      martin:
        healthcheck:
          test: ["CMD", "/usr/local/bin/martin", "health", "--url", "http://127.0.0.1:8080/health"]
    ```

!!! note
    Docker Compose does not restart unhealthy containers.
    To monitor and restart unhealthy containers, you can use [Docker Autoheal](https://github.com/willfarrell/docker-autoheal).
