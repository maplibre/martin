---
description: Serving tiles through the OGC API - Tiles standard
icon: material/api
tags:
  - api
---

# OGC API - Tiles

Martin serves its tile sources through [OGC API - Tiles](https://docs.ogc.org/is/20-057/20-057.html) under `/ogc`.
A client that speaks the standard finds every source and its tiles from the landing page, without knowing Martin's own URLs.
The OGC API is part of the default build, through the `ogcapi` feature.

| URL                                                                                        | Description                                  |
|--------------------------------------------------------------------------------------------|----------------------------------------------|
| `/ogc`                                                                                     | Landing page                                 |
| `/ogc/conformance`                                                                         | The conformance classes Martin implements    |
| `/ogc/tileMatrixSets`                                                                      | The tile matrix sets of the served sources   |
| `/ogc/tileMatrixSets/{tileMatrixSetId}`                                                    | One tile matrix set                          |
| `/ogc/collections`                                                                         | Sources in a supported CRS, as collections   |
| `/ogc/collections/{collectionId}`                                                          | One collection                               |
| `/ogc/collections/{collectionId}/tiles`                                                    | The tilesets of a collection                 |
| `/ogc/collections/{collectionId}/tiles/{tileMatrixSetId}`                                  | Tileset metadata, with the tile URL template |
| `/ogc/collections/{collectionId}/tiles/{tileMatrixSetId}/{tileMatrix}/{tileRow}/{tileCol}` | A tile                                       |

## Collections

Each tile source in a supported CRS is a collection, and its source ID is the collection ID.
A collection's extent is the bounds of its source.
Raster sources are map tilesets and vector sources are vector tilesets.
A source is listed when its grid uses one of the CRSs of the built-in grids, which are EPSG 3857, 4326, 3395, 3035, 2193, 5041 and 5042.
Martin knows the units and axis order of those, so a grid in another CRS, such as a `simple` grid, is not listed.
When an alias has a source's ID, the collection is the source the alias serves, as it is for tiles.
An alias of several sources is not a collection.

## Tiles

An OGC tile URL names the row before the column.
So `/ogc/collections/{id}/tiles/WebMercatorQuad/{z}/{y}/{x}` serves the same tile as `/{id}/{z}/{x}/{y}`.
Compression, caching and empty tiles work as they do on `/{sourceID}/{z}/{x}/{y}`.

!!! important "Behavioral Differences for OGC compliance"
    This endpoint sends MVT tiles as `application/vnd.mapbox-vector-tile` instead of `application/x-protobuf` like our regular endpoint, to be OGC compliant.

## Tile matrix sets

The tile matrix sets are the [tile grids](tile-grids.md) of the served sources, and Web Mercator is always listed.
A grid from the OGC registry carries its registry URI.
Each origin is given in the axis order of its CRS.

## Conformance

`/ogc/conformance` declares these classes.

- OGC API - Tiles core, tileset, tilesets list, geodata tilesets and Mapbox Vector Tiles
- OGC API - Common core, landing page and JSON
- Two Dimensional Tile Matrix Set 2.0 and its JSON encoding

The OGC conformance test suite runs against Martin in CI.

## Reading Martin with GDAL

GDAL's OGC API driver reads Martin's vector tiles, with one layer per zoom level.

```bash
ogrinfo OGCAPI:http://localhost:3000/ogc/collections/world_cities -oo API=TILES
```
