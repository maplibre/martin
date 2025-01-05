## Running with Docker Compose

You can use example [`docker-compose.yml`](https://raw.githubusercontent.com/maplibre/martin/main/docker-compose.yml)
file as a reference

```yml
services:
  martin:
    image: ghcr.io/maplibre/martin:v0.13.0
    restart: unless-stopped
    ports:
      - "3000:3000"
    environment:
      - DATABASE_URL=postgresql://postgres:password@db/db
    depends_on:
      - db

  db:
    image: postgis/postgis:16-3.4-alpine
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

Official Docker image includes a `HEALTHCHECK` instruction which will be used by Docker Compose. Note that Compose won't restart unhealthy containers. To monitor and restart unhealthy containers you can use [Docker Autoheal](https://github.com/willfarrell/docker-autoheal).
