---
icon: material/palette
tags:
  - styles
  - configuration
---

# Style Sources

Martin will serve your styles as needed by MapLibre rendering libraries.

To edit these styles, we recommend using <https://maputnik.github.io/editor/>.

### API

Martin can serve [MapLibre Style Spec](https://maplibre.org/maplibre-style-spec/).
Currently, Martin will use any valid [`JSON`](https://json.org) file as a style,
but in the future, we may optimise Martin which may result in additional restrictions.

Use the `/catalog` API to see all the `<style_id>`s.

### Map Style

Use the `/style/<style_id>` API to get a `<style_id>`'s JSON content.

Changes or removals of styles are reflected immediately, but additions are not.
A restart of Martin is required to see new styles.

### Server-side raster tile rendering

!!! warning
    This feature is included in the default build on Linux.
    Its behaviour may change in patch releases.

    Limitations of our current implementation:

    - Rendering support is currently only available on Linux.
      To add support for macOS/Windows, please see <https://github.com/maplibre/maplibre-native-rs>.
    - Currently, martin does not cache style rendered requests and
    - does not support concurrency for this feature.

    We welcome contributions to help stabilise this feature!

We support generating a rasterised image for an XYZ tile of a given style.

To do so, you need to enable the feature in the configuration file:

```yaml
styles:
    rendering: true
```

After doing so, you can use the `/style/<style_id>/{z}/{x}/{y}.{filetype}` API to get a `<style_id>`'s rendered png/jpeg content.

### Static images

!!! info
    We currently do not have the same [capabilities as Tileserver-GL](https://tileserver.readthedocs.io/en/latest/endpoints.html#static-images) to layout images.
    We are working on adding this feature and are very open to contributions if you want to help!

!!! warning
    Static rendering shares the limitations listed above (Linux only, no caching, no concurrency).
    The HTTP shape may still change in patch releases.

Martin can render a single PNG/JPEG/WebP of a style at a chosen camera.
The same URL is served by two methods:

```http
GET  /style/{style_id}/static/{camera}/{size}.{ext}
POST /style/{style_id}/static/{camera}/{size}.{ext}
```

`GET` returns the base map alone.
`POST` additionally accepts a partial [MapLibre style](https://maplibre.org/maplibre-style-spec/)
in the body and applies those sources and layers on top of the base style
for that single render.
An empty or missing body is equivalent to `GET`.

#### Camera

The `{camera}` segment chooses what the image looks at:

| Form                            | Meaning                                              |
| ------------------------------- | ---------------------------------------------------- |
| `lon,lat,zoom`                  | Center at `(lon, lat)` and `zoom` (north up, flat).  |
| `lon,lat,zoom@bearing`          | Center + bearing in degrees (clockwise from north).  |
| `lon,lat,zoom@bearing,pitch`    | Center + bearing + pitch in degrees.                 |
| `minLon,minLat,maxLon,maxLat`   | Fit the given bounding box to the requested size.    |

!!! important
    The image is always rendered INSIDE of the requested size.
    So if the bbox is `[-10°,-1°,10°,1°]` and size `500x500` is requested, the image will be centered on 0,0 with a 500x500 box that is fully inside the bbox, so the left-top most pixel is approximately at `1°,-1°`.
    We will not expand the image outside the bbox.

#### Size and format

`{size}.{ext}` follows the `WIDTHxHEIGHT[@{scale}x].{ext}` pattern, e.g.
`800x600.png`, `400x300@2x.jpg`.
Allowed extensions are `png`, `jpg`, and `webp`.
Width and height are capped at 2048 px each; scale is capped at `4x`.

#### Overlay body (`POST`)

The overlay body is a strict subset of the
[MapLibre Style Spec](https://maplibre.org/maplibre-style-spec/) — what you
send is exactly what MapLibre will render.

```json
{
  "sources": {
    "route":   { "type": "geojson", "data": { "type": "FeatureCollection", "features": [/* … */] } },
    "markers": { "type": "geojson", "data": { "type": "FeatureCollection", "features": [/* … */] } }
  },
  "layers": [
    { "id": "route-line",  "type": "line",   "source": "route",
      "paint":  { "line-color": "#f00", "line-width": 3 },
      "layout": { "line-cap": "round" } },
    { "id": "marker-dots", "type": "circle", "source": "markers",
      "paint":  { "circle-color": "#00f", "circle-radius": 6 } },
    { "id": "above-water", "type": "fill",   "source": "route",
      "paint":  { "fill-color": "#0a0", "fill-opacity": 0.5 },
      "before": "label-place" }
  ]
}
```

Rules — anything else is rejected with `400 Bad Request`:

- **Top level:** only `sources` and `layers`.
- **Sources:** `type` must be `"geojson"`. `data` must be an inline
  GeoJSON object — strings/URLs are rejected, eliminating SSRF surface.
- **Layers:** `type` must be one of `fill`, `line`, `circle`. `id`,
  `source`, `type` are required; `source` must reference a parsed source.
  `before` is optional and refers to a layer id in the **base style**
  (overlay layer ids cannot collide with base ids — the server prepends
  `overlay:` to every overlay id before handing it to MapLibre).
- **Paint / layout values are literals only.** Data-driven expressions
  (`["interpolate", …]`, `["get", "name"]`, etc.) are rejected.

Supported paint and layout properties:

| Layer type | Paint properties                                                                                                  | Layout properties           |
| ---------- | ----------------------------------------------------------------------------------------------------------------- | --------------------------- |
| `fill`     | `fill-color`, `fill-opacity`, `fill-outline-color`                                                                | —                           |
| `line`     | `line-color`, `line-opacity`, `line-width`                                                                        | `line-cap`, `line-join`     |
| `circle`   | `circle-color`, `circle-opacity`, `circle-radius`, `circle-stroke-color`, `circle-stroke-opacity`, `circle-stroke-width` | —                           |

`line-cap` is one of `butt`, `round`, `square`; `line-join` is one of
`miter`, `bevel`, `round`. Colors accept any CSS color string.

##### Out of scope

- Data-driven expressions.
- Layer types beyond `fill` / `line` / `circle` (no `symbol`, `heatmap`,
  `raster`, `fill-extrusion`).
- URL-referenced source `data` (would need an allowlist + timeouts).
- Symbol/icon markers (use `circle` instead).

All examples below render the same camera (`/static/0,0,2/200x200.png`)
against the `maplibre_demo` style, so the visual differences come only from
the overlay body.

##### Line layer

<div class="grid" markdown>

```json hl_lines="14-18"
{
  "sources": {
    "s": { "type": "geojson", "data": {
      "type": "FeatureCollection",
      "features": [{
        "type": "Feature", "properties": {},
        "geometry": {
          "type": "LineString",
          "coordinates": [[-10.0, -10.0], [10.0, 10.0]]
        }
      }]
    }}
  },
  "layers": [{
    "id": "line", "type": "line", "source": "s",
    "paint": { "line-color": "#95BEFA", "line-width": 5 },
    "layout": { "line-cap": "round", "line-join": "round" }
  }]
}
```

![Pastel-blue diagonal stroke over the demo basemap](images/static-overlay/path_stroke.png){ width="100%" }

</div>

##### Fill layer

<div class="grid" markdown>

```json hl_lines="20-24"
{
  "sources": {
    "s": { "type": "geojson", "data": {
      "type": "FeatureCollection",
      "features": [{
        "type": "Feature", "properties": {},
        "geometry": {
          "type": "Polygon",
          "coordinates": [[
            [-10.0, -10.0],
            [10.0, -10.0],
            [10.0, 10.0],
            [-10.0, 10.0],
            [-10.0, -10.0]
          ]]
        }
      }]
    }}
  },
  "layers": [{
    "id": "fill", "type": "fill", "source": "s",
    "paint": { "fill-color": "red", "fill-opacity": 1.0 }
  }]
}
```

![Filled red square polygon](images/static-overlay/polygon_fill.png){ width="100%" }

</div>

##### Fill opacity (alpha blending)

<div class="grid" markdown>

```json hl_lines="22-31"
{
  "sources": {
    "left":  { "type": "geojson", "data": {
      "type": "FeatureCollection",
      "features": [{
        "type": "Feature", "properties": {},
        "geometry": { "type": "Polygon",
          "coordinates": [[[-40, -20], [10, -20], [10, 20], [-40, 20], [-40, -20]]] }
      }]
    }},
    "right": { "type": "geojson", "data": {
      "type": "FeatureCollection",
      "features": [{
        "type": "Feature", "properties": {},
        "geometry": { "type": "Polygon",
          "coordinates": [[[-10, -20], [40, -20], [40, 20], [-10, 20], [-10, -20]]] }
      }]
    }}
  },
  "layers": [
    { "id": "left-fill",  "type": "fill", "source": "left",
      "paint": { "fill-color": "#285DAA", "fill-opacity": 0.5 } },
    { "id": "right-fill", "type": "fill", "source": "right",
      "paint": { "fill-color": "#95BEFA", "fill-opacity": 0.5 } }
  ]
}
```

![Two semi-transparent brand-color rectangles overlapping](images/static-overlay/polygon_fill_opacity.png){ width="100%" }

</div>

##### Circle layer (markers)

<div class="grid" markdown>

```json hl_lines="14-17"
{
  "sources": {
    "s": { "type": "geojson", "data": {
      "type": "FeatureCollection",
      "features": [{
        "type": "Feature", "properties": {},
        "geometry": { "type": "Point", "coordinates": [0.0, 0.0] }
      }]
    }}
  },
  "layers": [{
    "id": "marker", "type": "circle", "source": "s",
    "paint": { "circle-color": "#285DAA", "circle-radius": 8 }
  }]
}
```

![Primary-colored circle marker at the equator/prime meridian](images/static-overlay/marker_color.png){ width="100%" }

</div>
