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
`POST` in addition accepts a [GeoJSON](https://datatracker.ietf.org/doc/html/rfc7946)
`FeatureCollection` in the body and draws the features on top of the base map.
An empty/missing body is equivalent to `GET`.
Overlay properties are inspired by [`simplestyle`s](https://github.com/mapbox/simplestyle-spec).

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

#### Overlay properties (`POST`)

Geometry support is currently:

- `Point` / `MultiPoint` -> filled circle marker.
- `LineString` / `MultiLineString` -> stroked path.
- `Polygon` / `MultiPolygon` -> outer ring + any interior rings (holes),
  filled + stroked.
- `GeometryCollection` -> each child geometry inherits the feature's properties.

| Property         | Applies to              | Default     | Notes                                                                                |
| ---------------- | ----------------------- | ----------- | ------------------------------------------------------------------------------------ |
| `stroke`         | LineString, Polygon     | `"#555555"` | Any CSS color. On polygons the default is the resolved `fill` instead.               |
| `stroke-opacity` | LineString, Polygon     | `1.0`       | Multiplied with any alpha already encoded in `stroke`.                               |
| `stroke-width`   | LineString, Polygon     | `2.0`       | Pixels at the rendered scale; rounded line caps/joins.                               |
| `fill`           | Polygon                 | `"#555555"` | Any CSS color.                                                                       |
| `fill-opacity`   | Polygon                 | `0.6`       | Multiplied with `fill`'s own alpha.                                                  |
| `marker-color`   | Point                   | red         | Any CSS color. Renders as a filled circle (no SDF/icon support yet).                 |

All examples below render the same camera (`/static/0,0,2/200x200.png`)
against the `maplibre_demo` style, so the visual differences come only from
the overlay body.

##### `stroke` + `stroke-width`

<div class="grid" markdown>

```json hl_lines="8-11"
{
  "features": [
    {
      "geometry": {
        "coordinates": [[-10.0, -10.0], [10.0, 10.0]],
        "type": "LineString"
      },
      "properties": {
        "stroke": "#95BEFA",
        "stroke-width": 5
      },
      "type": "Feature"
    }
  ],
  "type": "FeatureCollection"
}
```

![Pastel-blue diagonal stroke over the demo basemap](images/static-overlay/path_stroke.png){ width="100%" }

</div>

##### `fill` (Polygon)

<div class="grid" markdown>

```json hl_lines="14-17"
{
  "features": [
    {
      "geometry": {
        "coordinates": [[
          [-10.0, -10.0],
          [10.0, -10.0],
          [10.0, 10.0],
          [-10.0, 10.0],
          [-10.0, -10.0]
        ]],
        "type": "Polygon"
      },
      "properties": {
        "fill": "red",
        "fill-opacity": 1.0
      },
      "type": "Feature"
    }
  ],
  "type": "FeatureCollection"
}
```

![Filled red square polygon](images/static-overlay/polygon_fill.png){ width="100%" }

</div>

##### `fill-opacity` (alpha blending)

<div class="grid" markdown>

```json hl_lines="5-9"
{
  "features": [
    {
      "geometry": {
        "coordinates": [[[-40, -20], [10, -20], [10, 20], [-40, 20], [-40, -20]]],
        "type": "Polygon"
      },
      "properties": {
        "fill": "#285DAA",
        "fill-opacity": 0.5,
        "stroke-opacity": 0
      },
      "type": "Feature"
    },
    {
      "geometry": {
        "coordinates": [[[-10, -20], [40, -20], [40, 20], [-10, 20], [-10, -20]]],
        "type": "Polygon"
      },
      "properties": {
        "fill": "#95BEFA",
        "fill-opacity": 0.5,
        "stroke-opacity": 0
      },
      "type": "Feature"
    }
  ],
  "type": "FeatureCollection"
}
```

![Two semi-transparent brand-color rectangles overlapping](images/static-overlay/polygon_fill_opacity.png){ width="100%" }

</div>

##### `marker-color` (Point)

<div class="grid" markdown>

```json hl_lines="5-7"
{
  "features": [
    {
      "geometry": { "coordinates": [0.0, 0.0], "type": "Point" },
      "properties": {
        "marker-color": "#285DAA"
      },
      "type": "Feature"
    }
  ],
  "type": "FeatureCollection"
}
```

![Primary-colored circle marker at the equator/prime meridian](images/static-overlay/marker_color.png){ width="100%" }

</div>

When `marker-color` is omitted, the marker falls back to the simplestyle-spec
default (red):

![Default red circle marker](images/static-overlay/marker_default.png)
