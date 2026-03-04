# Reverse Proxies

Martin **can run without** a reverse proxy.

Doing so has a few downsides:

- Martin does not support HTTPS connections (TLS termination).
- We do not check `HOST`-headers - we just serve on a port.
  This means anybody can point their dns record to your server and serve to all requests going to the port Martin is running on.
  Using a reverse proxy makes this abuse obvious.
- Martin only supports a simple in-memory caching.
  If you need more advanced caching options, you can use a reverse proxy with custom rules.
  You may for example only want to cache zoom `0..10`.
  Here are some reverse proxy options:
    - [Nginx](https://nginx.org/)
    - [Varnish](https://varnish-cache.org/)
    - [Apache](https://httpd.apache.org/)
- You may need to host more than just tiles/resources on the domain name.
- Martin has a fixed public API, but your site may require a different structure.
  For example, you may want to serve tiles from `/source?z=z&x=x&y=y`.
