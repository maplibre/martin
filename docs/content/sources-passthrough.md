---
icon: material/transit-connection-variant
tags:
  - passthrough
  - proxy
  - tile-sources
  - configuration
---

# Passthrough Sources

A `passthrough` source proxies tiles from an upstream HTTP tile server through Martin.
Martin fetches each tile from the upstream URL and serves the bytes verbatim, preserving the upstream `Content-Encoding`.
The rest of Martin's pipeline - [conversion between MVT and MLT](config-file/index.md#postprocessing) and tile [caching](config-file/index.md) - is applied on top, exactly as for any other source.

Use it to:

- put Martin's cache, headers, and MLT conversion in front of an existing tile server
- serve an upstream that requires an API key without leaking the key to browsers
- spread tile requests across several mirror upstreams

Unlike file sources, passthrough sources have no `paths:` to glob - each source names an upstream directly.

## Run Martin with configuration file

Passthrough sources are only available via the [configuration file](config-file/index.md); there is no CLI shorthand.
Each entry under `passthrough.sources` maps the source ID Martin serves under to an upstream.

```yaml
passthrough:
  sources:
    # Shorthand: a `{z}/{x}/{y}` URL template.
    osm: https://tile.openstreetmap.org/{z}/{x}/{y}.png

    # Shorthand: a TileJSON document URL. Its tile URLs, zoom range, bounds,
    # and attribution are read from the document.
    hosted: https://demotiles.maplibre.org/tiles/tiles.json

    # Shorthand: a list of URL templates, to spread requests across mirrors.
    mirrored:
      - https://a.example.com/{z}/{x}/{y}.pbf
      - https://b.example.com/{z}/{x}/{y}.pbf

    # Detailed object form, for headers, timeouts, and metadata.
    secure:
      url: https://api.example.com/{z}/{x}/{y}
      # HTTP headers sent with every upstream request. Values support `${ENV_VAR}` substitution.
      headers:
        Authorization: ${API_TOKEN}
      # Per-request timeout. Accepts human-readable values like "30s" or "1m". Defaults to "30s".
      timeout: 30s
      # Explicit tile format. When unset it is detected from the URL extension,
      # the upstream TileJSON, or the response itself.
      format: mvt
      # TileJSON metadata advertised for template upstreams.
      minzoom: 0
      maxzoom: 14
      bounds: [-180.0, -85.0511, 180.0, 85.0511]
      attribution: '© Example'
```

## Upstream forms

A source value under `passthrough.sources` is one of:

| Form | Example | Notes |
|------|---------|-------|
| URL template | `https://tile.osm.org/{z}/{x}/{y}.png` | A single `{z}/{x}/{y}` template. |
| TileJSON URL | `https://example.org/tiles.json` | A lone non-template URL is treated as a [TileJSON](https://github.com/mapbox/tilejson-spec) document; tiles, zoom, bounds, and attribution come from the document. |
| List of templates | `[https://a…/{z}/{x}/{y}, https://b…/{z}/{x}/{y}]` | Requests are spread across the mirrors. |
| Object | `{ url: …, headers: … }` | The detailed form below. |

### Detailed object fields

| Field | Applies to | Description |
|-------|-----------|-------------|
| `url` | all | Upstream `{z}/{x}/{y}` template(s) or a single TileJSON document URL. |
| `headers` | all | HTTP headers sent with every request (e.g. `Authorization`). Values support `${ENV_VAR}` substitution. |
| `timeout` | all | Per-request timeout, e.g. `30s`, `1m`. Defaults to `30s`. |
| `format` | all | Explicit tile format override (e.g. `mvt`, `png`). Detected when unset. |
| `minzoom` / `maxzoom` | templates only | Zoom range advertised in the served TileJSON. |
| `bounds` | templates only | Geographic bounds advertised in the served TileJSON. |
| `attribution` | templates only | Attribution advertised in the served TileJSON. |
| `cache` | all | Zoom-level bounds for tile caching. |
| `convert_to_mlt` / `convert_to_mvt` | all | Per-source [MVT/MLT conversion](config-file/index.md#postprocessing) overrides. |

!!! note
    `minzoom`, `maxzoom`, `bounds`, and `attribution` are only used for URL-template upstreams.
    For a TileJSON upstream, that metadata is read from the upstream document instead.

## Type-level conversion defaults

Alongside `sources`, the `passthrough` section accepts `convert_to_mlt` and `convert_to_mvt` keys that
apply to every passthrough source. They override the global defaults and are themselves overridden by a
per-source setting. See [Postprocessing](config-file/index.md#postprocessing) for what these conversions do.

```yaml
passthrough:
  convert_to_mlt: {}
  sources:
    osm: https://tile.openstreetmap.org/{z}/{x}/{y}.png
```

!!! warning
    Every requested tile results in an upstream HTTP fetch unless it is served from Martin's cache.
    Point passthrough sources only at upstreams you are authorised to proxy, and be mindful of their rate limits and usage policies.
