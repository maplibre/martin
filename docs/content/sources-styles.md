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
`POST` additionally accepts a GeoJSON `FeatureCollection` in the body, with
styling on each feature's `properties`, and overlays those features on top
of the base style for that single render.
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

The overlay body is a GeoJSON `FeatureCollection`. Each feature carries its
style on `properties` — either as
[simplestyle](https://github.com/mapbox/simplestyle-spec) aliases or as
canonical [MapLibre Style Spec](https://maplibre.org/maplibre-style-spec/)
property names. Both vocabularies are accepted on the same feature.

```json
{
  "type": "FeatureCollection",
  "features": [
    {
      "type": "Feature",
      "geometry": { "type": "LineString",
        "coordinates": [[-10, 5], [10, 5]] },
      "properties": { "line-color": "#f00", "line-width": 3, "line-cap": "round" }
    },
    {
      "type": "Feature",
      "geometry": { "type": "Point", "coordinates": [0, 0] },
      "properties": { "marker-color": "#00f", "marker-size": "medium" }
    },
    {
      "type": "Feature",
      "geometry": { "type": "Polygon",
        "coordinates": [[[-5,-5],[5,-5],[5,5],[-5,5],[-5,-5]]] },
      "properties": { "fill": "#0a0", "fill-opacity": 0.5 }
    }
  ]
}
```

How features become layers:

- **Point / MultiPoint** → one `circle` layer.
- **LineString / MultiLineString** → one `line` layer.
- **Polygon / MultiPolygon** → a `fill` layer (unless only stroke
  properties are set), plus a `line` layer when any stroke property is
  present (so a polygon with `stroke` gets an outline of `stroke-width`).
- `GeometryCollection` and features with `geometry: null` are silently
  skipped.

Anything the server can't make sense of is rejected with `400 Bad
Request`: a malformed body, a non-`FeatureCollection` top-level type, an
invalid CSS color, a non-numeric width, an unknown `line-cap`/`line-join`
enum value, or a `marker-size` that isn't `small`/`medium`/`large`.
Unknown property keys on `feature.properties` are **silently ignored**, so
GeoJSON files that already carry application metadata (`id`, `name`,
`title`, `description`, …) work without modification.

##### Supported style properties

Set either the simplestyle alias or the MapLibre canonical name;
if both are present the canonical name wins.

| Simplestyle alias    | MapLibre canonical    | Applies to            | Default        |
| -------------------- | --------------------- | --------------------- | -------------- |
| `marker-color`       | `circle-color`        | Point                 | `#7e7e7e`      |
| `marker-size`        | `circle-radius`       | Point                 | `8` (`medium`) |
| —                    | `circle-opacity`      | Point                 | `1`            |
| —                    | `circle-stroke-color` | Point                 | _unset_        |
| —                    | `circle-stroke-opacity` | Point               | _unset_        |
| —                    | `circle-stroke-width` | Point                 | _unset_        |
| `stroke`             | `line-color`          | LineString / Polygon  | `#555555`      |
| `stroke-opacity`     | `line-opacity`        | LineString / Polygon  | `1`            |
| `stroke-width`       | `line-width`          | LineString / Polygon  | `2`            |
| —                    | `line-cap`            | LineString / Polygon  | _MapLibre_     |
| —                    | `line-join`           | LineString / Polygon  | _MapLibre_     |
| `fill`               | `fill-color`          | Polygon               | `#555555`      |
| `fill-opacity`       | `fill-opacity`        | Polygon               | `0.6`          |
| —                    | `fill-outline-color`  | Polygon               | _unset_        |

`marker-size` is the enum `small`/`medium`/`large`, mapping to
`circle-radius` `6` / `8` / `10`. `line-cap` is one of `butt`, `round`,
`square`; `line-join` is one of `miter`, `bevel`, `round`. Colors accept
any CSS color string.

Range checks are not enforced — `*-opacity` values outside `0..=1` and
negative widths are passed through to MapLibre verbatim. Simplestyle's
informational `title` and `description` properties are accepted and
ignored.

##### Out of scope

- Data-driven expressions on individual feature properties (each feature
  becomes its own GeoJSON source and layer with literal paint values).
- Layer types beyond `fill` / `line` / `circle` (no `symbol`, `heatmap`,
  `raster`, `fill-extrusion`).
- Externally-referenced GeoJSON URLs (every feature is inline).
- Symbol/icon markers — simplestyle's `marker-symbol` is not supported;
  use a `circle` instead.
- Z-ordering relative to base style layers — all overlay layers are drawn
  on top of the base style, in feature-array order.

All examples below render the same camera (`/static/0,0,2/200x200.png`)
against the `maplibre_demo` style, so the visual differences come only from
the overlay body.

##### Line

<div class="grid" markdown>

```json hl_lines="9-14"
{
  "type": "FeatureCollection",
  "features": [{
    "type": "Feature",
    "geometry": {
      "type": "LineString",
      "coordinates": [[-10.0, -10.0], [10.0, 10.0]]
    },
    "properties": {
      "line-color": "#95BEFA",
      "line-width": 5,
      "line-cap": "round",
      "line-join": "round"
    }
  }]
}
```

![Pastel-blue diagonal stroke over the demo basemap](images/static-overlay/line_color.png){ width="100%" }

</div>

##### Fill

<div class="grid" markdown>

```json hl_lines="12-15"
{
  "type": "FeatureCollection",
  "features": [{
    "type": "Feature",
    "geometry": {
      "type": "Polygon",
      "coordinates": [[
        [-10.0, -10.0], [10.0, -10.0],
        [10.0, 10.0], [-10.0, 10.0],
        [-10.0, -10.0]
      ]]
    },
    "properties": {
      "fill-color": "red",
      "fill-opacity": 1.0
    }
  }]
}
```

![Filled red square polygon](images/static-overlay/fill_color.png){ width="100%" }

</div>

##### Fill opacity (alpha blending)

<div class="grid" markdown>

```json hl_lines="9-10 18-19"
{
  "type": "FeatureCollection",
  "features": [
    {
      "type": "Feature",
      "geometry": { "type": "Polygon",
        "coordinates": [[[-40, -20], [10, -20], [10, 20], [-40, 20], [-40, -20]]] },
      "properties": {
        "fill-color": "#285DAA",
        "fill-opacity": 0.5
      }
    },
    {
      "type": "Feature",
      "geometry": { "type": "Polygon",
        "coordinates": [[[-10, -20], [40, -20], [40, 20], [-10, 20], [-10, -20]]] },
      "properties": {
        "fill-color": "#95BEFA",
        "fill-opacity": 0.5
      }
    }
  ]
}
```

![Two semi-transparent brand-color rectangles overlapping](images/static-overlay/fill_opacity.png){ width="100%" }

</div>

##### Circle (marker)

<div class="grid" markdown>

```json hl_lines="9-12"
{
  "type": "FeatureCollection",
  "features": [{
    "type": "Feature",
    "geometry": {
      "type": "Point",
      "coordinates": [0.0, 0.0]
    },
    "properties": {
      "circle-color": "#285DAA",
      "circle-radius": 8
    }
  }]
}
```

![Primary-colored circle marker at the equator/prime meridian](images/static-overlay/circle_color.png){ width="100%" }

</div>
