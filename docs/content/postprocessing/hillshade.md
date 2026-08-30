---
icon: material/image-filter-hdr
tags:
  - hillshade
  - raster
  - tile-sources
  - configuration
---

# Hillshade

A hillshade is the grey relief image a map overlays over its basemap, multiplying it into the colors underneath to make terrain look three-dimensional.
Martin can bake one from a source that serves *normal maps* in Mapzen's encoding - tiles whose pixels store which way the ground faces instead of a color - which **spares the client this work** and lets the result be **compressed further**.
See the left image below for the input and the right for the output of this postprocessing.

This is less flexible for the client.

<div class="grid" markdown>

![amazon normal-map tiles in mapzen encoding](https://elevation-tiles-prod.s3.amazonaws.com/normal/10/163/396.png){width=500}

![postprocessed tile](../images/postprocess/hillshade.webp){width=500}

</div>


## Configuration

Set `convert_to_hillshade` on the source serving the normal maps:

```yaml
passthrough:
  sources:
    terrain:
      url: https://elevation-tiles-prod.s3.amazonaws.com/normal/{z}/{x}/{y}.png
      maxzoom: 14
      convert_to_hillshade: auto
```

`GET /terrain/12/2100/1300` now returns a 512x512 lossless PNG of shaded relief, replacing the normal map it was baked from.

`auto` bakes with every default, a map of settings (e.g. `convert_to_hillshade: {altitude: 60}`) overrides said defaults, and `disabled` (the default for `convert_to_hillshade`) is the same as omitting the key.
It is settable per source only since an inherited value would also reach vector-only sources (such as `postgres`), which cannot be shaded.

!!! warning "Declare `maxzoom` on the source"
    Zoom limits are inherited from the source's `TileJSON`, and absent limits mean every zoom is valid.
    A source upstream typically stops at some zoom, so without a declared `maxzoom` Martin keeps requesting tiles past it unnecessarily.

## Normal maps ("Mapzen") are the only input

A normal map stores one [*surface normal*](https://en.wikipedia.org/wiki/Normal_%28geometry%29) (the direction the ground faces) per pixel.
The horizontal components in the red and green channels and elevation in alpha.
The gradient is already baked in, so shading needs no knowledge of the tile's ground resolution.

!!! note "Elevation formats such as DEM and Terrarium are not supported."
    Shading heights means differentiating them against meters-per-pixel at that latitude and zoom, and matching the source's own quantization - a separate feature rather than a decode step on this one.
    PRs welcome :wink:

## Settings

| Setting                   | Default | Range         | Effect                                                                       |
|---------------------------|---------|---------------|------------------------------------------------------------------------------|
| `azimuth`                 | `300`   | `0`-`360`     | Compass bearing the light shines from, in degrees clockwise from north.      |
| `altitude`                | `45`    | `0`-`90`      | Height of the light above the horizon. `90` is overhead and flattens relief. |
| `vertical_exaggeration`   | `2.5`   | `0`-`10`      | Scales the horizontal gradient before lighting. `1` is true-to-source.       |
| `contrast`                | `2.5`   | `0`-`10`      | Separation between lit and shadowed slopes.                                  |
| `elevation_scale`         | `0`     | `0`-`10`      | How much high terrain deepens the contrast. `0` shades all terrain alike.    |
| `toon_bands`              | `6`     | `0`-`32`      | Hard shading bands. Below `2`, shading is a smooth gradient.                 |
| `ambient`                 | `0.2`   | `0`-`1`       | Shadow floor, so shadows read as shaded rather than black.                   |
| `padding`                 | `0`     | `0`-`32`      | Apron width in pixels at 256-core scale.                                     |
| `format`                  | `png`   | `png`, `webp` | Output image format.                                                         |
| `allow_request_overrides` | `false` |               | Whether query parameters may override the lighting parameters.               |

Out-of-range values are rejected at startup, rather than surfacing later on some tiles.

The default light comes from the north-west by cartographic convention.
Terrain lit from the lower half of the compass reads as inverted to most people, with valleys appearing to bulge out of the map.

<<<<<<< HEAD
!!! tip "what does `toon_bands` actually do?"
    The defaults for `toon_bands` "squash" the relief into six hard bands, which keeps the shading readable once it is multiplied under a basemap.
    Set `toon_bands: 0` for a smooth gradient instead.
    `elevation_scale` is off by default, so a slope is shaded the same way whether it sits in a valley or on a summit; raise it to make high terrain read more strongly than low.
=======
The defaults bake a plain, smoothly shaded relief: the two stylistic knobs, `toon_bands` and `elevation_scale`, are off.
They are what a basemap usually wants, and they leave any particular house style as something you opt into rather than something you have to undo.
>>>>>>> 14625522 (spellchecking)

Both formats are lossless because a hillshade is multiplied over the basemap, where a lossy codec's ringing would land on flat terrain as visible blotches instead of being masked by photographic detail.
Lossless WebP is typically around a third smaller than PNG, and is the better choice where clients support it.

!!! tip "`padding` is only for clients that sample past tile edges"
    The tile is rendered larger than its nominal size, so a client whose sampler reads just outside a tile edge finds real data there rather than disagreeing with the neighboring tile.
    MapLibre samples within tile bounds and needs none, hence the `0` default and the exact 512x512 tile.

    Padding is expressed at 256-core scale and rescaled with the core, so `padding: 8` yields a 16-pixel apron per side and a 544x544 tile, which the client must crop.

## Per-request overrides

!!! warning "This is an expensive feature"
    Leave `allow_request_overrides` off in production.
    It is meant for internal development, where you want to figure out which parameters look best for your specific map.
    Hillshading is CPU-intensive.
    A hillshade for fixed settings never changes (+/- data changes), so it is near-perfectly cacheable at the edge.

With `allow_request_overrides: true`, a request may override any of the seven lighting parameters by name:

```text
GET /terrain/12/2100/1300?azimuth=90&toon_bands=0
```

`padding`, `format`, and `allow_request_overrides` itself are not overridable.
Values that are out of range or not a number are rejected with `400`, while query parameters that are not hillshade settings are ignored.

## Caching and cache-busting

Baking reads a 3x3 neighborhood (to avoid boundary seams) of normal maps, so nine tiles are read per uncached tile served.
We cache the normal maps rather than the baked output since an undecoded normal map is identical for every parameter combination and is read nine times over by neighboring tiles, whereas a baked tile is specific to one parameter set.

Each tile's `ETag` is derived from the nine tiles it read plus the settings it was baked with, so it moves whenever any input or any setting does.

!!! warning "An `ETag` cannot cut short a `max-age` a client is already holding."
    To force clients onto a re-tuned hillshade, serve it under a new source ID.
    There is deliberately no cache-busting query parameter: that would leave clients in charge of the server's correctness.

## Neighborhood handling

Missing neighbors are routine rather than an error: at the poles, at the edge of coverage, or when one read of nine fails.
A missing or unreadable neighbor is replaced by extending the center tile's nearest edge outward, which may create a seam but still serves the tile.

The projection is cylindrical in x, so tiles at the antimeridian get real neighbors from the far side of the map instead of a clamped edge.
It is not cylindrical in y, so a tile in the top or bottom row has three clamped slots.

!!! danger "A center tile that fails to decode is an error"
    Serving a blank core inside a `200 OK` would let a CDN hold nothing-shaped terrain for as long as its `max-age`, which is worse than a visible failure.
