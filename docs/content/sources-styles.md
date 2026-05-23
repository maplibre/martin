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

Martin can render a single PNG/JPEG of a style at a chosen camera. The endpoint is:

```text
GET /style/{style_id}/static/{camera}/{size}.{ext}
```

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
