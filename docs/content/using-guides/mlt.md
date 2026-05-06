# Serving MLT (MapLibre Tiles)

This guide explains how to configure Martin to convert MVT (Mapbox Vector Tiles) to [MLT (MapLibre Tiles)](https://github.com/maplibre/maplibre-tile-spec) format on the fly.

## What is MLT?

MLT is a compact, columnar tile format designed as a successor to MVT.
See the [MLT spec](https://github.com/maplibre/maplibre-tile-spec) for details.

Martin can convert MVT tiles to MLT at serve time, so you don't need to re-generate your tile archives or change your database functions.
If you generate your own tile archives, consider generating them using MLT instead since you save CPU cycles and latency this way.
The default for this is optimised for size, optimises for network size (!= low CPU usage) and can thus convert approximately 10k tiles/s.

!!! note "Prerequisites"

    Martin must be built with the `mlt` feature enabled and up to date.
    If the feature is not enabled, Martin will return an error when MLT conversion is requested.

## Quick Start

Add `convert_to_mlt: auto` in your config file to convert all MVT sources to MLT:

```yaml
convert_to_mlt: auto

# your existing source configuration
postgres:
  connection_string: postgresql://localhost/mydb
pmtiles:
  sources:
    basemap: /data/basemap.pmtiles
```

Sources that produce other formats (raster, etc.) are unaffected.

## Scoping MLT Conversion

You don't have to convert everything.
The `convert_to_mlt` key can be placed at three levels, and the most specific one wins entirely (see [Configuration File](../config-file/index.md) for details).

### Convert only PostgreSQL sources

```yaml
postgres:
  convert_to_mlt: auto
  connection_string: postgresql://localhost/mydb
```

### Convert a single source

```yaml
pmtiles:
  sources:
    basemap:
      path: /data/basemap.pmtiles
      convert_to_mlt: auto
    imagery:
      path: /data/imagery.pmtiles
      # no convert_to_mlt — served as-is
```

### Opt a single source out of MLT

If a higher level (global or source-type) enables MLT but you want one source
to keep serving MVT, set `convert_to_mlt: disabled` or `convert_to_mvt: disabled`.
The most-specific level wins, so this overrides any inherited `auto`.

```yaml
convert_to_mlt: auto              # default everywhere

pmtiles:
  sources:
    basemap:
      path: /data/basemap.pmtiles
      # Inherits global `auto` -> converted on Accept: MLT
    legacy:
      path: /data/legacy.pmtiles
      convert_to_mlt: disabled    # always served as MVT, even on Accept: MLT
```

## Tuning the Encoder

`convert_to_mlt: auto` uses "magic" default encoder settings, which work well for most data.
The `auto` preset is guaranteed to work on a reasonably new maplibre version, so for example `tessellation` will only be enabled if this has wider spread benefits.

If you need to tweak encoding behavior, provide an explicit configuration:

```yaml
convert_to_mlt:
  tessellate: true
  try_spatial_hilbert_sort: true
  allow_fsst: false
```

All fields are optional. Only the fields you specify override the defaults; unset fields keep their `mlt-core` default values.

| Field                      | When to change                                                                                              |
|----------------------------|-------------------------------------------------------------------------------------------------------------|
| `tessellate`               | Enable if your client supports pre-tessellated polygons and you benchmarked that this improves your usecase |
| `try_spatial_morton_sort`  | Disable if your data is already spatially ordered                                                           |
| `try_spatial_hilbert_sort` | Disable if Morton sort doesn't compress well for your data                                                  |
| `try_id_sort`              | Enable when features have sequential IDs and spatial sorting isn't beneficial                               |
| `allow_fsst`               | Disable to reduce search space                                                                              |
| `allow_fpf`                | Disable to reduce search space                                                                              |
| `allow_shared_dict`        | Disable to reduce search space                                                                              |

!!! note

    In most cases, `convert_to_mlt: auto` is the right choice.
    Only tune these settings if you've profiled your tiles and identified a specific bottleneck.

!!! warning

    MLT uses lightweight compressions (FastPFOR, FSST, ...), so combining it with heavyweight compression (e.g. gzip) removes most of the reasons for using it.
    Do this only if you have benchmarked that this actually makes your usecase better.
    If for example CPU usage is an issue, disable some sorting options that you have benchmarked to be ineffective.

## Serving MVT from MLT Sources

If you have tile archives that already contain MLT tiles (e.g. MBTiles or PMTiles generated with an MLT encoder), Martin can convert them back to MVT on the fly for clients that don't support MLT yet.

Add `convert_to_mvt: auto` at the global, source-type, or per-source level:

```yaml
convert_to_mvt: auto

mbtiles:
  sources:
    my_mlt_archive: /data/tiles.mbtiles
```

When a client requests tiles with `Accept: application/x-protobuf` (MVT) from a source that produces MLT, Martin will decode the MLT tile and re-encode it as MVT protobuf.

### Scoping

Like `convert_to_mlt`, the `convert_to_mvt` key can be placed at three levels and the most specific one wins:

```yaml
# Per-source
pmtiles:
  sources:
    mlt_archive:
      path: /data/mlt_tiles.pmtiles
      convert_to_mvt: auto
```

!!! note

    Currently `convert_to_mvt` only supports `auto`. There are no tunable encoder settings for MVT output since the protobuf format has a fixed encoding.
