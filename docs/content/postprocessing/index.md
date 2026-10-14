---
description: Transforming tiles before serving, such as MLT, hillshade and contours
tags:
  - configuration
---

# Postprocessing

Martin's postprocessing pipeline can transform tiles on the fly, before they are served.

Currently configurable:

- **`convert_to_mlt`** - encoder settings for MVT->MLT conversion (triggered by `Accept: application/vnd.maplibre-tile`). See the [MVT/MLT conversion guide](mlt.md).
- **`convert_to_mvt`** - enables MLT->MVT conversion (triggered by `Accept: application/x-protobuf` on an MLT source). Currently only supports `auto`.
- **`convert_to_hillshade`** - bakes a hillshade from a source serving Mapzen normal tiles. Unlike the two above, this is settable per source only, since it describes what a source serves rather than a server-wide policy.
- **`convert_to_contour`** - traces contour lines from a source serving Mapzen Terrarium elevation tiles, and serves them as MVT. Settable per source only, for the same reason. See the [contour guide](contour.md).

The two conversion keys can be set globally, per source type, or per source.
The most specific level that sets either key decides both, and levels are never merged.

```yaml
convert_to_mlt:
  tessellate: true

mbtiles:
  sources:
    roads:
      path: /data/roads.mbtiles

pmtiles:
  convert_to_mlt:
    allow_fsst: false
  sources:
    basemap:
      path: /data/basemap.pmtiles
    legacy:
      path: /data/legacy.pmtiles
      convert_to_mlt: disabled
    archive:
      path: /data/archive.pmtiles
      convert_to_mvt: auto
```

--8<-- "images/postprocess/cascade.svg"
