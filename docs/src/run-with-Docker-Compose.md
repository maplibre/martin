# Running with Docker Compose

You can use example [`docker-compose.yml`](https://raw.githubusercontent.com/maplibre/martin/main/docker-compose.yml) file as a reference

```yml
version: '3'

services:
  martin:
    image: ghcr.io/maplibre/martin:v0.7.0
    restart: unless-stopped
    ports:
      - "3000:3000"
    environment:
      - DATABASE_URL=postgresql://postgres:password@db/db
    depends_on:
      - db

  db:
    image: postgis/postgis:14-3.3-alpine
    restart: unless-stopped
    environment:
      - POSTGRES_DB=db
      - POSTGRES_USER=postgres
      - POSTGRES_PASSWORD=password
    volumes:
      - ./pg_data:/var/lib/postgresql/data
```

First, you need to start `db` service

```shell
docker-compose up -d db
```

Then, after `db` service is ready to accept connections, you can start `martin`

```shell
docker-compose up -d martin
```

By default, Martin will be available at [localhost:3000](http://localhost:3000/)
