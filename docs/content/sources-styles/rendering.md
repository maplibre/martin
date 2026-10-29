---
description: Rendering styles to raster tiles and static images
icon: material/image
tags:
  - styles
  - rendering
  - configuration
---

# Server-side raster tile rendering

Martin can render a [style](index.md) into raster images server-side:
as XYZ tiles, or as a single static image at a chosen camera.

!!! warning
    Rendering is **not** part of the default build.
    It relies on [MapLibre Native](https://github.com/maplibre/maplibre-native-ffi), which needs EGL and OpenGL ES at runtime.
    It renders on the GPU when an EGL driver for it is installed, and on the CPU with Mesa's `llvmpipe` otherwise.
    We only ship it where we control the runtime environment.
    You get it by either:

    - using the **`-full` Docker image** variant, tagged `:latest-full`, or the matching `-full` Linux-gnu release tarball, or
    - building from source with the `rendering` feature enabled (Linux only): `cargo install martin --features rendering`.

    The default Docker image, the default release binaries, and `cargo install martin` do **not** include rendering.

    Limitations of our current implementation:

    - Rendering support is currently only available on Linux.
      To add support for macOS/Windows, please see <https://github.com/maplibre/maplibre-native-ffi>.
    - Martin does not cache rendered requests.

To enable rendering, you need a build that includes it (see above) and to turn it on in the configuration file:

```yaml
styles:
    rendering: true
```

Renders run on a dedicated thread pool.
`rendering: true` sizes it from the logical CPU count, clamped to `2..=8`.
The long form sets the number of render threads explicitly, how many [renderers](#renderers-per-worker) each keeps, the [tile size](#tile-size), the [pixel ratios](#pixel-ratio) tiles and static images are served at, and the size of the [ambient cache](#ambient-cache):

```yaml
styles:
    rendering:
        enabled: true
        workers: 4
        # Renderers each worker keeps loaded, one per style and pixel ratio [default: 8]
        renderers_per_worker: 8
        # Width and height of XYZ tiles in pixels: 256 or 512 [default: 512]
        tile_size: 512
        # Whole-number @{n}x pixel ratios served, each 1 to 4 [default: [1, 2]]
        pixel_ratios: [1, 2]
        # MB of MapLibre Native's cache of fetched tiles, glyphs and sprites, 0 to disable [default: 50 MiB]
        ambient_cache_size_mb: 0
        # Indexed PNG palette; `false` keeps full-color RGBA [default: max_colors 128]
        png_palette:
            max_colors: 128
        # Quality per format: WebP 1 to 100 or `lossless`, JPEG 1 to 100 [default: 80 each]
        quality:
            webp: 80
            jpeg: 80
```

## Indexed (palette) PNG

Rendered PNG tiles and static images are indexed (palette) PNGs.
For map tiles they are about a quarter of the size of full-color RGBA, with no visible difference.
JPEG and WebP are not affected, they follow the [quality](#quality) setting.

Each image gets the smallest palette that stays close to the full-color render, up to `max_colors` (2 to 256, default 128).
Encoding a tile this way takes a few milliseconds of CPU.

```yaml
styles:
    rendering:
        enabled: true
        png_palette:
            max_colors: 64
```

Set `png_palette: false` for full-color RGBA PNGs, for example for imagery or styles with smooth gradients:

```yaml
styles:
    rendering:
        enabled: true
        png_palette: false
```

## Quality

Rendered WebP and JPEG tiles and static images are lossy at quality 80.
Each format has its own `quality`, as the two degrade differently at the same number.
Both are optional, and any value outside the ones below fails at startup.
PNG is not affected, see [indexed PNG](#indexed-palette-png).

- **`webp`** is 1 to 100, or `lossless`. Lossy WebP is encoded with [libwebp](https://developers.google.com/speed/webp).
  At quality 80, map tiles are about a sixth of the size of lossless WebP, with no visible difference.
  The alpha channel stays lossless.
- **`jpeg`** is 1 to 100. JPEG has no lossless mode.

```yaml
styles:
    rendering:
        enabled: true
        quality:
            webp: 90
            jpeg: 90
```

Lossless WebP keeps every pixel, for example for imagery or styles with smooth gradients:

```yaml
styles:
    rendering:
        enabled: true
        quality:
            webp: lossless
```

## Rendered XYZ tiles

We support generating a rasterized image for an XYZ tile of a given style.

After enabling rendering, you can use the `/style/<style_id>/{z}/{x}/{y}.{filetype}` API to get a `<style_id>`'s rendered png/jpeg/webp content.

### Tile size

Tiles are 512×512 px by default, the size MapLibre uses.
Set `tile_size: 256` to serve 256×256 px tiles, the size most other raster clients (such as Leaflet or OpenLayers) expect by default.
Any other value fails at startup.

### Pixel ratio

For high-density (retina) screens, add `@{n}x` after the row to draw the same tile at `n` times the pixels:
`/style/<style_id>/{z}/{x}/{y}@2x.png` is twice `tile_size` wide and high (1024×1024 px with the default 512), `@3x` three times.
`n` must be listed in `pixel_ratios` (`[1, 2]` unless configured, each 1 to 4): an unlisted ratio such as `@3x` is answered with `400 Bad Request`, and a malformed suffix such as `@0x` or `@1.5x` with `404 Not Found`.
A tile without a suffix counts as `@1x`, so `pixel_ratios: [2]` also refuses it.
Every listed ratio needs renderers of its own, so list only the ratios your clients ask for.

In Leaflet, this is the `{r}`-placeholder (`/style/<style_id>/{z}/{x}/{y}{r}.png`), which means that on a retina screen, you get the crisp map your users expect.

### Renderers per worker

Each render worker keeps a renderer for every style and pixel ratio it has been asked for, up to `renderers_per_worker` (8 unless configured).
Beyond that, the least recently used one is dropped, and loaded again the next time it is needed.
With `tile_size: 256`, the zoom 0 tile takes one more renderer per pixel ratio.

Reloading styles costs CPU on every request that misses, so set `renderers_per_worker` to at least the number of styles times the pixel ratios you serve.
Each renderer holds its own memory: lower `renderers_per_worker` or list fewer `pixel_ratios` to reduce memory usage.

### Ambient cache

MapLibre Native keeps the tiles, glyphs and sprites its renderers fetch over the network in a cache of its own, in memory and shared by all of them, up to `ambient_cache_size_mb` MB (MapLibre Native's default of 50 MiB unless configured).
Every fetched resource is compressed and stored there, and every fetch looks it up first.
A cached resource spares a request only while its `Cache-Control` says it is fresh, and Martin sends none unless `cache_control` is set in the [configuration file](../config-file/index.md).
When the style's sources are Martin's own, the tiles are already a local read away, so `ambient_cache_size_mb: 0` saves that work.
Keep it for styles that fetch from other servers.

### Shader cache

On the CPU, Mesa (`llvmpipe`) compiles every new renderer's shaders and caches them on disk, in `$XDG_CACHE_HOME` or `$HOME/.cache`.
Without that cache, creating a renderer, at startup or after one was [dropped](#renderers-per-worker), takes several times longer.

When neither directory is writable, as in a read-only container, Martin caches shaders in a private directory under `$TMPDIR` (`/tmp` unless set) until it exits, and logs that it did.
If that is read-only too, it logs a warning and Mesa compiles shaders again for every renderer.
For a read-only container, mount a small `tmpfs` at `/tmp` (the cache stays under a few MB), or set `MESA_SHADER_CACHE_DIR` to a writable volume to keep the cache across restarts:

```bash
docker run --read-only --tmpfs /tmp ... ghcr.io/maplibre/martin:2.0.0-beta.4
```

Martin leaves `MESA_SHADER_CACHE_DIR` and `MESA_SHADER_CACHE_DISABLE` alone when they are set.

## Logs

MapLibre logs its own messages under the `maplibre_native` target, which `RUST_LOG` hides unless it names that target.
`RUST_LOG=martin=info,maplibre_native=warn` shows MapLibre's warnings and errors, such as a tile or a glyph range that failed to load.

## Static images

!!! info
    We currently do not have the same [capabilities as Tileserver-GL](https://tileserver.readthedocs.io/en/latest/endpoints.html#static-images) to layout images.
    We are working on adding this feature and are very open to contributions if you want to help!

!!! warning
    Static rendering shares the limitations listed above (Linux only, no caching).

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

### Camera

The `{camera}` segment chooses what the image looks at:

--8<-- "images/styles/camera.svg"

!!! info
    The whole bounding box is always visible.
    So if the bbox is `[-10°,-1°,10°,1°]` and size `500x500` is requested, the image is centered on `0,0` and spans the full 20° of longitude, with extra map above and below the bbox.
    A bounding box always renders north up and flat.

### Size and format

`{size}.{ext}` follows the `WIDTHxHEIGHT[@{scale}x].{ext}` pattern, e.g.
`800x600.png`, `400x300@2x.jpg`.
Allowed extensions are `png`, `jpg`, and `webp`.
Width and height are capped at 2048 px each.
The scale is a whole number listed in [`pixel_ratios`](#pixel-ratio) (`1` if omitted): an unlisted scale is answered with `400 Bad Request`, and a fractional or zero scale such as `@1.5x` or `@0x` with `404 Not Found`.

### Overlay body (`POST`)

The overlay body is a GeoJSON `FeatureCollection`. Each feature carries its
style on `properties` using
[MapLibre Style Spec](https://maplibre.org/maplibre-style-spec/) paint/layout
property names (`circle-*`, `line-*`, `fill-*`). Any property left unset falls
back to MapLibre's own default.

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
      "properties": { "circle-color": "#00f", "circle-radius": 8 }
    },
    {
      "type": "Feature",
      "geometry": { "type": "Polygon",
        "coordinates": [[[-5,-5],[5,-5],[5,5],[-5,5],[-5,-5]]] },
      "properties": { "fill-color": "#0a0", "fill-opacity": 0.5 }
    }
  ]
}
```

How features become layers:

- **Point / MultiPoint** -> one `circle` layer.
- **LineString / MultiLineString** -> one `line` layer.
- **Polygon / MultiPolygon** -> a `fill` layer (unless only `line-*`
  properties are set), plus a `line` layer when any `line-*` property is
  present (so a polygon with `line-color` gets an outline of `line-width`).
- `GeometryCollection` and features with `geometry: null` are silently
  skipped.

Anything the server can't make sense of is rejected with `400 Bad
Request`: a malformed body, a top-level `type` that isn't
`FeatureCollection`, a member `type` that isn't `Feature`, an invalid CSS
color, a non-numeric width, or an unknown `line-cap`/`line-join` enum value.
Unknown property keys on `feature.properties` are **silently ignored**, so
GeoJSON files that already carry application metadata (`id`, `name`,
`title`, `description`, …) work without modification.

#### Supported style properties

Every property is optional; an unset property uses the
[MapLibre Style Spec](https://maplibre.org/maplibre-style-spec/) default.

| MapLibre property       | Applies to            | MapLibre default     |
| ----------------------- | --------------------- | -------------------- |
| `circle-color`          | Point                 | `#000000`            |
| `circle-opacity`        | Point                 | `1`                  |
| `circle-radius`         | Point                 | `5`                  |
| `circle-stroke-color`   | Point                 | `#000000`            |
| `circle-stroke-opacity` | Point                 | `1`                  |
| `circle-stroke-width`   | Point                 | `0`                  |
| `line-color`            | LineString / Polygon  | `#000000`            |
| `line-opacity`          | LineString / Polygon  | `1`                  |
| `line-width`            | LineString / Polygon  | `1`                  |
| `line-cap`              | LineString / Polygon  | `butt`               |
| `line-join`             | LineString / Polygon  | `miter`              |
| `fill-color`            | Polygon               | `#000000`            |
| `fill-opacity`          | Polygon               | `1`                  |
| `fill-outline-color`    | Polygon               | matches `fill-color` |

`line-cap` is one of `butt`, `round`, `square`; `line-join` is one of
`miter`, `bevel`, `round`. Colors accept any CSS color string.

Range checks are not enforced - `*-opacity` values outside `0..=1` and
negative widths are passed through to MapLibre verbatim.

#### Out of scope

- Data-driven expressions on individual feature properties (each feature
  becomes its own GeoJSON source and layer with literal paint values).
- Layer types beyond `fill` / `line` / `circle` (no `symbol`, `heatmap`,
  `raster`, `fill-extrusion`).
- Externally-referenced GeoJSON URLs (every feature is inline).
- Symbol/icon markers (no `symbol` layer) - use a `circle` instead.
- Z-ordering relative to base style layers - all overlay layers are drawn
  on top of the base style, in feature-array order.

All examples below render the same camera (`/static/0,0,2/200x200.png`)
against the `maplibre_demo` style, so the visual differences come only from
the overlay body.

#### Line

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

![Pastel-blue diagonal stroke over the demo basemap](../images/static-overlay/line_color.png){ width="100%" }

</div>

#### Fill

<div class="grid" markdown>

```json hl_lines="13-16"
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

![Filled red square polygon](../images/static-overlay/fill_color.png){ width="100%" }

</div>

#### Fill opacity (alpha blending)

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

![Two semi-transparent brand-color rectangles overlapping](../images/static-overlay/fill_opacity.png){ width="100%" }

</div>

#### Circle (marker)

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

![Primary-colored circle marker at the equator/prime meridian](../images/static-overlay/circle_color.png){ width="100%" }

</div>
