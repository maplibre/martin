# Using with NGINX

You can run Martin behind NGINX proxy, so you can cache frequently accessed tiles and reduce unnecessary pressure on the database. Here is an example `docker-compose.yml` file that runs Martin with NGINX and PostgreSQL.

```yml
version: '3'

services:
  nginx:
    image: nginx:alpine
    restart: unless-stopped
    ports:
      - "80:80"
    volumes:
      - ./cache:/var/cache/nginx
      - ./nginx.conf:/etc/nginx/nginx.conf:ro
    depends_on:
      - martin

  martin:
    image: maplibre/martin:v0.7.0
    restart: unless-stopped
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

You can find an example NGINX configuration file [here](https://github.com/maplibre/martin/blob/main/demo/frontend/nginx.conf).

## Rewriting URLs

If you are running Martin behind NGINX proxy, you may want to rewrite the request URL to properly handle tile URLs in [TileJSON](40-using.md#source-tilejson).

```nginx
location ~ /tiles/(?<fwd_path>.*) {
    proxy_set_header  X-Rewrite-URL $uri;
    proxy_set_header  X-Forwarded-Host $host:$server_port;
    proxy_set_header  X-Forwarded-Proto $scheme;
    proxy_redirect    off;

    proxy_pass        http://martin:3000/$fwd_path$is_args$args;
}
```

## Caching tiles

You can also use NGINX to cache tiles. In the example, the maximum cache size is set to 10GB, and caching time is set to 1 hour for responses with codes 200, 204, and 302 and 1 minute for responses with code 404.

```nginx
http {
  ...
  proxy_cache_path  /var/cache/nginx/
                    levels=1:2
                    max_size=10g
                    use_temp_path=off
                    keys_zone=tiles_cache:10m;

  server {
    ...
    location ~ /tiles/(?<fwd_path>.*) {
        proxy_set_header        X-Rewrite-URL $uri;
        proxy_set_header        X-Forwarded-Host $host:$server_port;
        proxy_set_header        X-Forwarded-Proto $scheme;
        proxy_redirect          off;

        proxy_cache             tiles_cache;
        proxy_cache_lock        on;
        proxy_cache_revalidate  on;

        # Set caching time for responses
        proxy_cache_valid       200 204 302 1h;
        proxy_cache_valid       404 1m;

        proxy_cache_use_stale   error timeout http_500 http_502 http_503 http_504;
        add_header              X-Cache-Status $upstream_cache_status;

        proxy_pass              http://martin:3000/$fwd_path$is_args$args;
    }
  }
}
```

You can find an example NGINX configuration file [here](https://github.com/maplibre/martin/blob/main/nginx.conf).
