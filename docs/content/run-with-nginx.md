# Using with NGINX

You can run Martin behind NGINX proxy, so you can cache frequently accessed tiles with custom logic.
Here is an example `docker-compose.yml` file that runs Martin with NGINX and PostgreSQL.

```compose
--8<-- "files/compose.nginx.yaml"
```

You can [find an example NGINX configuration file here](https://github.com/maplibre/martin/blob/main/demo/frontend/nginx.conf).

### Rewriting URLs

If you are running Martin behind NGINX proxy, you may want to rewrite the request URL to properly handle tile URLs in [TileJSON](using.md#source-tilejson).

```nginx
location ~ /tiles/(?<fwd_path>.*) {
    proxy_set_header  X-Rewrite-URL $uri;
    proxy_set_header  X-Forwarded-Host $host:$server_port;
    proxy_set_header  X-Forwarded-Proto $scheme;
    proxy_redirect    off;

    proxy_pass        http://martin:3000/$fwd_path$is_args$args;
}
```
## Serving Fonts

If your font names contain spaces (e.g. `Open Sans Regular`), NGINX may
decode the `%20` in the URL into a literal space before forwarding it,
causing Martin to return an HTTP 400 error. Add this location block to
prevent that:

```nginx
location ~ /font/(?<fwd_path>.*) {
    proxy_pass http://martin:3000/font/$fwd_path$is_args$args;
}
```

### Caching tiles

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

You can [find an example NGINX configuration file here](https://github.com/maplibre/martin/blob/main/demo/frontend/nginx.conf).
