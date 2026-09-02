---
tags:
  - tile-sources
  - configuration
---

# Tile Grids

Martin serves tiles on the Web Mercator grid: EPSG:3857, one square tile at zoom 0, four per zoom level after that. Some data is published on other grids, in a national projection such as New Zealand's NZTM2000, on a polar stereographic grid around a pole, or on a coordinate reference system for another planet. A source can be served on such a grid instead, and a client that renders that grid, like [MapLibre GL JS with a custom projection](https://github.com/maplibre/maplibre-gl-js/pull/8287) or OpenLayers with a custom tile grid, places the tiles where they belong.

A tile grid here is a square power-of-two quad grid, the "quad" family of the [OGC Two Dimensional Tile Matrix Set](https://docs.ogc.org/is/17-083r4/17-083r4.html) standard: a coordinate reference system, the top-left corner of the zoom-0 tile, its side, and how many tiles zoom 0 has. Every zoom level splits each tile into four, columns grow east and rows grow south. Two grids are built in and always available: `WebMercatorQuad`, the default, and `WorldCRS84Quad`, plain longitude and latitude with two tiles at zoom 0.

## Defining a grid

Grids are named under the top-level `tile_grids` key. A PostgreSQL table or function, an MBTiles file or a PMTiles file refers to one by name with `tile_grid`, and a PostgreSQL connection can set a default for all of its sources.

```yaml
tile_grids:
  NZTM2000Quad:
    crs: EPSG:2193
    origin: [-3260586.7284, 10438190.1652]
    extent_at_zoom0: 10018754.1714

postgres:
  connection_string: postgres://postgres@localhost/db
  tables:
    nz_roads:
      schema: public
      table: roads
      srid: 2193
      geometry_column: geom
      tile_grid: NZTM2000Quad
    nz_roads_mercator:
      schema: public
      table: roads
      srid: 2193
      geometry_column: geom

mbtiles:
  sources:
    nz_basemap:
      path: /data/nz_basemap.mbtiles
      tile_grid: NZTM2000Quad
```

- `crs` is the grid's coordinate reference system as `AUTHORITY:CODE`, or `simple` for plain planar units (see [Floor plans and game maps](#floor-plans-and-game-maps)). `EPSG` codes need nothing else. Any other authority must exist in the database's `spatial_ref_sys` table under that `auth_name` and `auth_srid` (see [Beyond EPSG](#beyond-epsg)).
- `origin` is the top-left corner `[x, y]` of the zoom-0 tile, in CRS units.
- `extent_at_zoom0` is the side of one zoom-0 tile, in CRS units.
- `matrix_at_zoom0` is how many tile `[columns, rows]` zoom 0 has, `[1, 1]` unless set. `WorldCRS84Quad` and most planetary geographic grids are `[2, 1]`, the OGC UTM quads are `[1, 2]`. A grid that is two by two at zoom 0 is one by one at zoom 1, so start counting there instead.

Take the three numbers from the tile matrix set document published with the data, not from the CRS's area of use. Tile matrix set documents may list corners northing-first, as LINZ does for NZTM2000Quad, so order them `[x, y]`. Some services count zoom levels from a 2x2 level: NASA GIBS publishes its polar grids that way, so their "level 0" is zoom 1 here, and the zoom-0 tile is the whole 8,388,608 m square with origin `[-4194304, 4194304]`.

The same three numbers configure MapLibre GL JS, where `addProjection` takes them as `tileMatrix: {origin, extentAtZoom0}`.

Two grids for one table are two sources, as in the example above. There is no per-request grid selection, so a source's URL and its cache entries never change with the grid.

## What a grid changes

For a PostgreSQL table, the tile query uses the grid's zoom-0 square as the `bounds` of `ST_TileEnvelope` and encodes geometry in the grid's CRS. A table stored in the grid's CRS needs no reprojection at all, which is less work per feature than serving it on Web Mercator. A table stored in another CRS is transformed by PostGIS, as always.

A PostgreSQL function is trusted to produce tiles on the grid it names, and Martin only advertises the grid. The same goes for MBTiles and PMTiles files, which are served as stored. Declaring a grid changes the `TileJSON` and the catalog, never the bytes.

A request for a tile the grid does not have, a third column at zoom 0 of a two-wide grid or any tile beyond the edge, answers 404. This holds for Web Mercator too, where such tiles used to answer an empty 204.

The `TileJSON` of a source on a non-default grid carries the grid in a `tileGrid` key, with the same field names MapLibre GL JS uses:

```json
"tileGrid": {
  "id": "NZTM2000Quad",
  "crs": "EPSG:2193",
  "origin": [-3260586.7284, 10438190.1652],
  "extentAtZoom0": 10018754.1714
}
```

The `/catalog` entry of such a source names the grid in `tile_grid`. Sources on different grids cannot be combined into one composite source.

## Poles and other curved edges

When a table is stored in a different CRS than its grid, Martin transforms each tile's envelope into the table's CRS to search the spatial index. The envelope is densified first, so edges that curve in the table's CRS still cover the tile. What no transform can recover is a pole that lies inside the envelope rather than on its boundary, as it does on the zoom-0 tile of a polar grid. Martin detects that at startup and warns for every table in another CRS. Store polar data in the grid's CRS, and the search needs no transform at all.

The `bounds` a table advertises are computed in WGS84, as `TileJSON` requires. When PostGIS cannot transform the table's CRS into WGS84, as with a planetary one, Martin skips that computation and logs why. Set `bounds` in the config if the `TileJSON` should carry them.

## Floor plans and game maps

A grid with `crs: simple` has no geographic meaning at all. Its units are whatever the data is in, pixels of a scanned plan or metres of a level. This is the server side of Leaflet's `CRS.Simple` and of the `simple` projection in MapLibre GL JS.

```yaml
tile_grids:
  FloorPlan:
    crs: simple
    origin: [0, 1000]
    extent_at_zoom0: 1000

postgres:
  tables:
    rooms:
      schema: public
      table: rooms
      geometry_column: geom
      tile_grid: FloorPlan
```

The table stores its geometry with SRID 0, which is what PostGIS calls coordinates without a coordinate reference system, and Martin adopts it as such for a simple grid. Nothing is ever transformed. No `bounds` are computed, since there is no WGS84 to express them in.

## Beyond EPSG

PostGIS knows a coordinate reference system by its row in `spatial_ref_sys`. Systems that PostGIS does not ship, such as those the International Astronomical Union defines for other planets, are added as rows, and a grid names them by the row's `auth_name` and `auth_srid`:

```sql
INSERT INTO spatial_ref_sys (srid, auth_name, auth_srid, proj4text)
VALUES (949900, 'IAU_2015', 49900, '+proj=longlat +a=3396190 +b=3376200 +no_defs +type=crs');
```

```yaml
tile_grids:
  MarsGeographic:
    crs: IAU_2015:49900
    origin: [-180, 90]
    extent_at_zoom0: 360
```

Martin looks up the SRID once when it connects, and refuses to start if the row is missing.

## What stays on Web Mercator

COG, GeoJSON and DuckDB sources produce their tiles in Web Mercator and cannot be declared to be on another grid. A COG is read as the Web Mercator pyramid it was written as, GeoJSON is tiled in Web Mercator on the way in, and the DuckDB queries transform to EPSG:3857. Each of those would need its own work to serve another grid, and none of it is this feature.

Martin never reprojects tiles. Tiles are generated on the grid, or served as stored.
