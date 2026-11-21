# Shapefile fixtures

- `ocean.zip`: a stand-in for [osmdata.openstreetmap.de](https://osmdata.openstreetmap.de/data/water-polygons.html)'s
  `water-polygons-split-3857.zip`, with the same layout, `.prj` and fields: one square in the Atlantic, 8 pixels wide
  at zoom 0, whose `x` and `y` are 1 and 2. Written from the workspace root by

  ```bash
  uv run --with pyshp python tests/fixtures/shapefile/ocean.py
  ```
