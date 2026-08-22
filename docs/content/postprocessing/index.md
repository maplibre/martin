---
tags:
  - configuration
---

# Postprocessing

Martin's postprocessing pipeline can transform tiles on the fly, before they are served.

Currently configurable:

- **`convert_to_mlt`** - encoder settings for MVT->MLT conversion (triggered by `Accept: application/vnd.maplibre-tile`). See the [MVT/MLT conversion guide](mlt.md).
- **`convert_to_mvt`** - enables MLT->MVT conversion (triggered by `Accept: application/x-protobuf` on an MLT source). Currently only supports `auto`.
- **`convert_to_hillshade`** - bakes a hillshade from a source serving Mapzen normal tiles. Unlike the two above, this is settable per source only, since it describes what a source serves rather than a server-wide policy.

The two conversion keys can appear at three levels.
The most specific level wins entirely (no merging between levels):

1. **Global** - applies to all sources
2. **Source-type** - applies to all sources of that type (e.g. all PMTiles sources)
3. **Per-source** - applies to a single source

```yaml
# Global: default encoder settings for any source whose tiles get converted
convert_to_mlt: auto
convert_to_mvt: auto

postgres:
  connection_string: postgresql://localhost/mydb
  # Source-type: override the encoder config for all PG sources
  convert_to_mlt: auto
  tables:
    my_table:
      # Per-source: this table uses the default MLT encoder config
      convert_to_mlt: auto
    no_mlt_table:
      # Per-source: explicitly opt out
      # Even if the client requests MLT, this source is served as MVT.
      convert_to_mlt: disabled
mbtiles: # gets global default
  - some/file.mbtiles
```
