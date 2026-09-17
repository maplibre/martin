---
icon: simple/nginx
tags:
  - deployment
  - reverse-proxy
  - nginx
---

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

### Authenticating requests

Martin does not authenticate requests, so put the check in NGINX.
The [`auth_request`](https://nginx.org/en/docs/http/ngx_http_auth_request_module.html) directive sends a subrequest to a service of your choice before each tile request is proxied.
A `2xx` reply lets the request through and a `401` or `403` reply is returned to the client as is.
The subrequest carries the headers of the original request, so the service can validate a JWT in the `Authorization` header, check a cookie, or look up an API key from the query string.
The check runs before the cache lookup, so cached tiles are protected too.

```nginx
location = /auth {
    internal;
    proxy_pass              http://auth:8080/check;
    proxy_pass_request_body off;
    proxy_set_header        Content-Length "";
    proxy_set_header        X-Original-URI $request_uri;
}

location ~ /tiles/(?<fwd_path>.*) {
    auth_request      /auth;

    proxy_set_header  X-Rewrite-URL $uri;
    proxy_set_header  X-Forwarded-Host $host:$server_port;
    proxy_set_header  X-Forwarded-Proto $scheme;
    proxy_redirect    off;

    proxy_pass        http://martin:3000/$fwd_path$is_args$args;
}
```

The service behind `/auth` can be a few lines in any language or a ready-made one such as [oauth2-proxy](https://oauth2-proxy.github.io/oauth2-proxy/).
On the client side, MapLibre GL JS can attach the credential to every tile request through the [`transformRequest`](https://maplibre.org/maplibre-gl-js/docs/API/type-aliases/RequestTransformFunction/) option.

### Caching tiles

You can also use NGINX to cache tiles.
In the example, the maximum cache size is set to 10GB, and caching time is set to 1 hour for responses with codes 200, 204, and 302 and 1 minute for responses with code 404.

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
