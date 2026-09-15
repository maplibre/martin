---
icon: material/grid
tags:
  - tile-sources
  - configuration
---

# Tile Grids

A tile grid describes how a map is cut into square tiles.
It tells a client where the tile at `z/x/y` lies in the world and how big it is.

By default, Martin uses the Web Mercator grid.
This is the grid Google Maps, Mapbox and MapLibre use.
It covers the whole world (minus the poles) with one tile at zoom 0, four at zoom 1, sixteen at zoom 2 and so on.

Some data is published on a different grid.
Examples are national grids like New Zealand's NZTM2000, grids centred on a pole, or grids for other planets.
Such data only keeps its shape and accuracy on its own grid, so Martin can serve a source on that grid instead.
Clients like [MapLibre GL JS](https://github.com/maplibre/maplibre-gl-js/pull/8287) (via `addProjection` with a `tileMatrix`) or [OpenLayers](https://openlayers.org/en/latest/apidoc/module-ol_tilegrid_TileGrid-TileGrid.html) (via a custom tile grid) can then render it.

## What makes up a grid

A grid needs a **coordinate reference system** (CRS).
A CRS defines what a coordinate such as `[x, y]` means (which units it is in, where `0, 0` lies, and how the curved earth was flattened onto the plane).
Most CRS have a code in the EPSG registry, for example `EPSG:4326` for plain longitude and latitude and `EPSG:3857` for Web Mercator.
The coordinates of a grid are in the units of its CRS, usually metres or degrees.

Martin follows the "quad" grids of the [OGC Two Dimensional Tile Matrix Set](https://docs.ogc.org/is/17-083r4/17-083r4.html) standard.
"Quad" means every zoom level cuts each tile into four equal squares.
A grid is defined by:

- the CRS its coordinates are in,
- the top-left corner of the zoom-0 tile,
- the side length of that tile, and
- how many tiles zoom 0 has.

Columns (`x`) count from the left edge to the right, rows (`y`) from the top edge down.

Two grids are built in:

- `WebMercatorQuad` is **the default** in Martin and in all modern web maps (MapLibre, Google, Mapbox, ...).
  Because of the underlying math, this should be what you reach for on performant, global maps.
- `WorldCRS84Quad` uses plain longitude and latitude as coordinates.
  The world is twice as wide as it is tall in degrees, so zoom 0 has two tiles side by side.

## Defining a tile grid

Grids are named under the top-level `tile_grids` key.
PostgreSQL tables and functions, MBTiles files and PMTiles files refer to a grid by name via `tile_grid`.
A PostgreSQL connection can set a default `tile_grid` for all of its sources.

```yaml hl_lines="1-5 15 26"
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

We support these settings:

| key               | default  | meaning                                                                                                  |
|-------------------|----------|----------------------------------------------------------------------------------------------------------|
| `crs`             | required | The CRS as `AUTHORITY:CODE`, for example `EPSG:2193`, or `simple` for grids without a location on earth. |
| `origin`          | required | Top-left corner `[x, y]` of the zoom-0 tile, in the units of the CRS.                                    |
| `extent_at_zoom0` | required | Side length of one zoom-0 tile, in the units of the CRS.                                                 |
| `matrix_at_zoom0` | `[1, 1]` | Number of tile `[columns, rows]` at zoom 0.                                                              |

`EPSG` codes work out of the box.
Other authorities need a row in the database, see [Non-EPSG coordinate reference systems](#non-epsg-coordinate-reference-systems).
Grids with `crs: simple` are covered in [Non-geographic grids](#non-geographic-grids-floor-plans-and-pcmobile-game-maps).

`WorldCRS84Quad` and most grids for other planets have `[2, 1]` tiles at zoom 0.
The OGC UTM quads have `[1, 2]`.
A grid that is `[2, 2]` at zoom 0 is `[1, 1]` at zoom 1, so define it starting there.

!!! tip "Copy the grid from its publisher"
    Whoever publishes data on a grid usually also publishes the grid itself, often as a "tile matrix set" JSON document.
    Copy `origin` and `extent_at_zoom0` from that document.
    Do not compute them from the region the CRS is meant for, the grid is usually larger or shifted.

!!! warning "Tile matrix set documents list the corner in the axis order of the CRS"
    For EPSG:2193 and EPSG:4326 that is northing or latitude first, so the LINZ document gives the NZTM2000Quad corner as `[10438190.1652, -3260586.7284]`.
    Martin always takes `[x, y]`, so swap the two values for such a CRS.

!!! note "Some services count zoom levels differently"
    NASA GIBS starts counting its polar grids at a level that already has 2x2 tiles.
    Their "level 0" is zoom 1 in Martin.
    Martin's zoom-0 tile is then the whole 8,388,608 m square with origin `[-4194304, 4194304]`.

!!! note "One grid per source"
    Serving one table on two grids means two sources, as in the example above.
    A client cannot pick a grid per request.
    A source's URL and its cache entries never change with the grid.

## Serving a source on a grid

For a PostgreSQL table, Martin computes the square each tile covers from the grid and asks PostGIS for the geometry inside it.
The geometry in the tile is encoded in the grid's CRS.
If the table already stores its coordinates in that CRS, PostGIS only has to clip.
If the table uses a different CRS, PostGIS converts the coordinates on every request.

PostgreSQL functions, MBTiles files and PMTiles files are served as they are.
Declaring a grid changes the `TileJSON` and the catalog, never the tile bytes.

!!! warning "Functions and archives are not checked"
    Martin trusts functions, MBTiles files and PMTiles files to produce tiles on the grid they declare and only advertises that grid.
    If the declared grid does not match the tiles, clients will draw them in the wrong place.

!!! note "Tiles outside the grid answer 404"
    This includes a third column at zoom 0 of a two-wide grid.
    This also holds for Web Mercator, where such tiles used to answer an empty 204.

The `TileJSON` of a source on a non-default grid has a `tileGrid` key.
It uses the same field names as MapLibre GL JS:

```json
"tileGrid": {
  "id": "NZTM2000Quad",
  "crs": "EPSG:2193",
  "origin": [-3260586.7284, 10438190.1652],
  "extentAtZoom0": 10018754.1714
}
```

The `/catalog` entry of such a source names the grid in `tile_grid`.
Sources on different grids cannot be combined into one composite source.

`martin-cp` copies a source on its own grid.
Without `--bbox` it copies the whole grid.
With `--bbox`, the bounds are `min_x,min_y,max_x,max_y` in the units of the grid's CRS, not longitude and latitude.
The `TileJSON` written into the archive contains the `tileGrid` key.
The archive can then be served back with the same `tile_grid` declared.

## Tables stored in another CRS

If a table stores its coordinates in a different CRS than its grid, Martin converts each tile square into the table's CRS to find the matching rows.
A straight edge in one CRS often becomes a curve in another.
Martin therefore adds extra points along the edges before converting, so the search area covers the whole curved edge.

!!! note "Tiles across the antimeridian"
    Longitude and latitude, and Web Mercator, cut the world open at 180 degrees.
    A tile that reaches across that line has no single bounding box in such a CRS.
    For a table stored in a geographic CRS or in Web Mercator, Martin searches the whole width of the world at the height of that tile instead, so nothing is missed.
    Such tiles take longer than the tiles around them.
    Tables in other CRS with such a cut are not detected.
    Store the data in the grid's CRS to avoid both.

!!! warning "Polar grids"
    A tile that contains a pole cannot be converted this way, because the pole has no single position in most CRS.
    This happens on the zoom-0 tile of a polar grid.
    Martin detects this at startup and warns for every affected table.
    Store polar data in the grid's CRS to avoid the conversion entirely.

!!! note "`bounds` for other planets"
    `TileJSON` requires the `bounds` of a source as longitude and latitude (WGS84), so Martin converts them.
    If PostGIS cannot convert the table's CRS to longitude and latitude, Martin leaves `bounds` out and logs why.
    This is the case for CRS of other planets.
    Set `bounds` in the config if the `TileJSON` should have them.

## Non-geographic grids (floor plans and PC/mobile game maps)

A grid with `crs: simple` has no location on earth.
Its units are whatever the data is in, for example pixels of a scanned plan or metres of a game level.
This is the server side of Leaflet's `CRS.Simple` and of MapLibre GL JS's `simple` projection.

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

The table stores its geometry with SRID 0.
An SRID is the number PostGIS tags a geometry column with to say which CRS it is in, and 0 means "none".
Nothing is ever converted.
No `bounds` are computed, since there is no longitude and latitude to express them in.

## Non-EPSG coordinate reference systems

PostGIS keeps the CRS it knows in its `spatial_ref_sys` table.
It ships with every EPSG code.
Any other CRS has to be inserted as a row first.
An example are the CRS the International Astronomical Union defines for other planets.

```sql
INSERT INTO spatial_ref_sys (srid, auth_name, auth_srid, proj4text)
VALUES (949900, 'IAU_2015', 49900, '+proj=longlat +a=3396190 +b=3376200 +no_defs +type=crs');
```

- `srid` is the number PostGIS uses internally. Pick any free one.
- `auth_name` and `auth_srid` form the `AUTHORITY:CODE` name that a grid refers to.
- `proj4text` is the CRS definition in [PROJ](https://proj.org/) syntax.

```yaml
tile_grids:
  MarsGeographic:
    crs: IAU_2015:49900
    origin: [-180, 90]
    extent_at_zoom0: 360
```

## Sources that only support Web Mercator

COG, GeoJSON and DuckDB sources always produce Web Mercator tiles.
They cannot be declared to be on another grid.

- A COG is read as the Web Mercator pyramid it was written as.
- GeoJSON is tiled in Web Mercator on the way in.
- The DuckDB queries convert to EPSG:3857.

Martin never converts finished tiles from one grid to another.
Tiles are either generated on the grid or served as stored.
