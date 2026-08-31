---
icon: material/chart-timeline-variant
tags:
  - contour
  - vector
  - tile-sources
  - configuration
---

# Contours

A contour map draws lines joining points of equal elevation, so a reader can see how steep the ground is and read a height straight off the map.
Martin can trace them from a source that serves *elevation tiles* in Mapzen's Terrarium encoding - tiles whose pixels store a height in meters rather than a color - and serve the result as a vector tile.
See the left image below for the input and the right for the output of this postprocessing.

Unlike [hillshade](hillshade.md), the output is **vector**, not raster: the client styles the lines, labels them, and can pick which ones to show at which zoom.
The right image below is therefore one styling of the traced tile, drawn by Martin's own [server-side rendering](../sources-styles/rendering.md).

!!! tip "Pair this with [MLT](mlt.md)"
    A contour tile is thousands of long linestrings over one layer with two columns, one of them a boolean and the other a small integer that repeats across every line at the same height.
    That is close to the best case for MLT's columnar encoding, so `convert_to_mlt` pays off more here than on a typical vector source.

<div class="grid" markdown>

![amazon terrarium elevation tiles in mapzen encoding](https://elevation-tiles-prod.s3.amazonaws.com/terrarium/12/2135/1457.png){width=500}

![the contours traced from it, with major lines bolded and labelled with their elevation](../images/postprocess/contour.webp){width=500}

</div>

## Configuration

Set `convert_to_contour` on the source serving the elevation tiles:

```yaml
passthrough:
  sources:
    elevation:
      url: https://elevation-tiles-prod.s3.amazonaws.com/terrarium/{z}/{x}/{y}.png
      maxzoom: 14
      convert_to_contour: auto
```

`GET /elevation/12/2100/1300` now returns an MVT tile with a single `contour` layer, replacing the elevation tile it was traced from.

`auto` traces with every default, a map of settings (e.g. `convert_to_contour: {major_interval: 10}`) overrides said defaults, and `disabled` (the default for `convert_to_contour`) is the same as omitting the key.
It is settable per source only since an inherited value would also reach vector sources and normal-map sources, which carry no elevation to trace.

!!! warning "Declare `maxzoom` on the source"
    Zoom limits are inherited from the source's `TileJSON`, and absent limits mean every zoom is valid.
    A source upstream typically stops at some zoom, so without a declared `maxzoom` Martin keeps requesting tiles past it unnecessarily.

!!! note "Normal ("mapzen") maps are not currently supported"
    A Terrarium pixel decodes to meters as `(R * 256 + G + B / 256) - 32768`.
    Mapzen *normal* tiles (the input [hillshade](hillshade.md) takes) store a direction rather than a height and cannot be contoured currently.

## Output

Every feature is a `LineString` in one layer, carrying two tags:

| Tag     | Type    | Value                                                      |
|---------|---------|------------------------------------------------------------|
| `ele`   | integer | Height of the line, in whole configured `elevation_units`.  |
| `major` | boolean | `true` for every `major_interval`-th line.                  |

`major` exists so a style can draw those lines bolder and label only those, which is the usual cartographic convention.
Both tags are typed for size: a boolean and a small integer encode far tighter than strings and doubles, especially under [MLT](mlt.md).

!!! note "Elevations are rounded to whole units"
    A contour interval below `1` therefore lands several lines on the same reported `ele`.
    Contour intervals that fine are not readable on a map anyway, so this is not a practical limit.
    Elevations are carried as 16-bit, which a Terrarium pixel is by construction; only `feet` below the deepest ocean trench exceed that, and those clamp.

## Settings

| Setting                    | Default    | Range                | Effect                                                                              |
|----------------------------|------------|----------------------|--------------------------------------------------------------------------------------|
| `resolution`               | `10`       | `1`-`64`             | Sampling step the isolines are traced at. Higher is smoother.                       |
| `major_interval`           | `5`        | `0`-`64`             | Every n-th line gets `major: true`. `0` leaves every line `false`.                  |
| `simplification_tolerance` | `10`       | `0`-`100`            | Douglas-Peucker tolerance. Higher means smaller tiles and coarser lines.            |
| `min_feature_length`       | `50`       | `0`-`10000`          | Contour lines shorter than this are dropped.                                        |
| `fetch_margin`             | `32`       | `0`-`64`             | Apron in source pixels traced past the tile edge, then transformed back out.        |
| `zoom_intervals`           | see below  | `zoom: interval` map | Contour interval per zoom, in `elevation_units`. `0` disables contours at that zoom. |
| `elevation_units`          | `meters`   | `meters`, `feet`     | Units intervals are declared in and elevations are reported in.                     |
| `filtered_threshold`       | `0`        | number, `disabled`   | An elevation whose contour is suppressed.                                           |
| `layer_name`               | `contour`  |                      | MVT layer the features are written into.                                            |
| `extent`                   | `4096`     | `1`-`16384`          | MVT tile extent.                                                                    |
| `allow_request_overrides`  | `false`    |                      | Whether query parameters may override the tracing parameters.                       |

Out-of-range values are rejected at startup.

### Zoom intervals

Contours are only useful over a narrow band of scales: too coarse an interval says nothing, too fine a one is unreadable and enormous.
`zoom_intervals` maps a starting zoom to the interval used from there until the next entry, so the map thins out as you zoom out:

```yaml
convert_to_contour:
  zoom_intervals:
    0: 0      # no contours at all below z4
    4: 400
    6: 200
    8: 150
    10: 100
    12: 50
```

That is the default for `elevation_units: meters`.
For `feet` the default is `0` below z5, then `1000`, `500`, `400`, `250`, and `100` from z13.

### Filtering a threshold

`filtered_threshold` defaults to `0`, sea level.

A flat tile sits above exactly one threshold, so tracing it produces a single line around the tile's own edge - which over open ocean means every water tile answers with a rectangle.
Suppressing the sea-level line removes that artifact.
Set it to another elevation to suppress a different one, or to `disabled` to draw them all.

## Per-request overrides

!!! warning "This is an expensive feature"
    Leave `allow_request_overrides` off in production.
    It is meant for internal development, where you want to figure out which parameters look best for your specific map.
    Tracing contours is CPU-intensive.
    Contours for fixed settings never change (+/- data changes), so they are near-perfectly cacheable at the edge.

With `allow_request_overrides: true`, a request may override any of the five numeric tracing parameters by name:

```text
GET /elevation/12/2100/1300?resolution=16&simplification_tolerance=0
```

`zoom_intervals`, `elevation_units`, `filtered_threshold`, `layer_name`, `extent`, and `allow_request_overrides` itself are not overridable.
Values that are out of range or not a number are rejected with `400`, while query parameters that are not contour settings are ignored.

## Caching and cache-busting

Tracing reads a 3x3 neighborhood of elevation tiles, so nine tiles are read per uncached tile served.
We cache the elevation tiles rather than the traced output since an undecoded elevation tile is identical for every parameter combination and is read nine times over by neighboring tiles, whereas a traced tile is specific to one parameter set.

Each tile's `ETag` is derived from the nine tiles it read plus the settings it was traced with, so it moves whenever any input or any setting does.

!!! warning "An `ETag` cannot cut short a `max-age` a client is already holding."
    To force clients onto re-tuned contours, serve them under a new source ID.
    There is deliberately no cache-busting query parameter: that would leave clients in charge of the server's correctness.

## Neighborhood handling

Contours are traced across a 3x3 neighborhood with a `fetch_margin` apron, then shifted back so only the center tile's extent is encoded.
Without it, a line crossing a tile edge would stop short of its continuation in the next tile and the seam would be visible at every boundary.

The neighborhood is assembled the same way [hillshade](hillshade.md#neighborhood-handling) assembles its own:

- x wraps at the antimeridian, since the projection is cylindrical there and the neighbors are real tiles.
- y stops at the poles, where there is no tile at all, and the slot is edge-clamped from the center instead.
- A missing or undecodable *neighbor* degrades one seam and is clamped over.
- An undecodable *center* is an error, since serving a clamped-from-nothing trace would look like genuinely contour-free terrain.
- An absent *center* serves no content (a `204`), for the same reason.
