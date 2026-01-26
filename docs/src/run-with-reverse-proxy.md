# Reverse Proxies

Martin can run without a reverse proxy.

Doing so has a few downsides:

- Martin does not support HTTPS connections (TLS termination).
- We do not check `HOST`-headers - we just serve on a port.
  This means anybody can point their dns record to your server and serve to all requests going to the port Martin is running on.
  Using a reverse proxy makes this abuse obvious.
- Martin only supports a simple in-memory caching.
  If you need more advanced caching options, you can use a reverse proxy like [Nginx](https://nginx.org/), [Varnish](https://varnish-cache.org/), or [Apache](https://httpd.apache.org/) with custom rules.
  For example, you may choose to only cache zoom 0..10.
- You may need to host more than just tiles at a single domain name.
- Martin has a fixed public API, but your site may require a different structure, e.g. serving tiles with from a sub-path like `/tiles/source/z/x/y`.

## Configuring Base URL for Proxies and CloudFront

When Martin runs behind a reverse proxy, load balancer, or CloudFront, the TileJSON responses need to reference the public-facing URL rather than Martin's internal URL. Use the `base_url` configuration option to set the scheme and host that should be used in TileJSON tile URLs.

### Command Line

```bash
martin --base-url https://example.com <other-options>
```

### Configuration File

```yaml
base_url: https://example.com
# or with a path prefix
base_url: https://tiles.example.com/path
```

### Example Use Cases

**CloudFront + Lambda**: When Martin runs on AWS Lambda behind CloudFront, clients should request tiles from the CloudFront URL, not the Lambda function URL:

```yaml
base_url: https://example.com
```

**Proxy with Observability**: When running behind a proxy for observability, billing, analytics, or caching:

```yaml
base_url: https://proxy.example.com
```

With this configuration, TileJSON responses will contain tile URLs like `https://example.com/source/{z}/{x}/{y}` instead of the internal service URL.

**Note**: The `base_url` option only affects the URLs in TileJSON responses. It does not change Martin's API endpoints or listening address.

