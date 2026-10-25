---
description: Serving tiles through the OGC API - Tiles standard
icon: material/api
tags:
  - api
---

# OGC API - Tiles

Martin also serves your tiles through the [OGC API - Tiles](https://docs.ogc.org/is/20-057/20-057.html) standard, under `/ogc`.
You get exactly the same tiles as from the regular [Martin endpoints](using.md), just at different URLs and with OGC metadata instead of TileJSON.

!!! tip "Do you need this?"
    Probably not.
    Map libraries like MapLibre work best with the regular endpoints.
    Use `/ogc` when a tool only speaks OGC API - Tiles, like GDAL's [OGC API driver](#reading-martin-with-gdal).

The OGC API is included in the default build, as the `ogcapi` feature.

## How a client finds a tile

An OGC client doesn't need to know how Martin's URLs look.
You give it the landing page, `/ogc`, and it follows links from there, picking each one by its `rel`, until it has a URL template for the tiles.

--8<-- "images/ogc/walk.svg"

That's four requests before the first tile, where a MapLibre map with TileJSON needs one.

The landing page also links `/ogc/api`, the OGC API part of Martin's [OpenAPI 3.1](https://spec.openapis.org/oas/v3.1.1) definition, for clients that generate requests from it.
Clients that weigh HTML above JSON in `Accept`, as browsers do, get it as a [Swagger UI](https://swagger.io/tools/swagger-ui/) page instead, which loads Swagger UI from the jsDelivr CDN.
OGC API's OpenAPI conformance classes ask for OpenAPI 3.0, so Martin does not declare them.

## Tile URLs

The thing that trips people up: OGC puts the row before the column.
An OGC tile URL ends in `{z}/{y}/{x}`, not `{z}/{x}/{y}`.

--8<-- "images/ogc/tile-url.svg"

The tile matrix set in the URL has to be the source's [tile grid](tile-grids.md).
For most sources that's `WebMercatorQuad`, and anything else gets a `404`.

Compression, caching and empty tiles behave just like on the regular tile endpoint.

## Compared to the Martin endpoints

|                     | Martin endpoints                                 | OGC API                                                           |
|---------------------|--------------------------------------------------|-------------------------------------------------------------------|
| list of sources     | [`/catalog`](using.md#catalog)                   | `/ogc/collections`                                                |
| source metadata     | [TileJSON](using.md#source-tilejson)             | tileset, at `/ogc/collections/{sourceID}/tiles/{tileMatrixSetId}` |
| tile                | `/{sourceID}/{z}/{x}/{y}`                        | `/ogc/collections/{sourceID}/tiles/{tileMatrixSetId}/{z}/{y}/{x}` |
| MVT content type    | `application/x-protobuf`                         | `application/vnd.mapbox-vector-tile`, as the standard requires    |
| composite sources   | [`/{source1},…,{sourceN}`](sources-composite.md) | not available                                                     |
| grids in other CRSs | served                                           | not listed, see [below](#which-sources-show-up)                   |
| style               | [`/style/{styleID}`](sources-styles/index.md)    | `/ogc/styles/{styleID}`, see [Styles](#styles)                    |

## Which sources show up

Every tile source shows up as a collection, with its source ID as the collection ID.
Vector sources get a vector tileset, raster sources a map tileset.

There are two exceptions:

- **Sources on an unusual grid.**
  The standard needs the units and axis order of each grid's CRS, and Martin only knows those for the CRSs of the [built-in grids](tile-grids.md#built-in-grids): EPSG 3857, 4326, 3395, 3035, 2193, 5041 and 5042.
  A source on a `simple` grid, or on a custom grid in another CRS, won't be listed.
- **Composite sources and aliases.**
  A collection is always a single source, so `/a,b` has no OGC counterpart and [aliases](sources-composite.md#source-aliases) aren't listed under their own name.
  If an alias has the same name as a source, the collection serves the alias, just like the tile endpoint does.
  If that alias combines several sources, the source drops out of the list.

## Tile matrix sets

"Tile matrix set" is OGC's name for a [tile grid](tile-grids.md).
`/ogc/tileMatrixSets` lists the grids your sources use, plus `WebMercatorQuad`, which is always there.
Each one links to its full definition in the [Two Dimensional Tile Matrix Set 2.0](https://docs.ogc.org/is/17-083r4/17-083r4.html) JSON format: the origin, cell size and number of rows and columns at every zoom.

Grids from the OGC registry also carry their official registry `uri`.
Origins are written in the CRS's own axis order, which for EPSG 3035 and 2193 means northing first.

## Styles

`/ogc/styles` lists your [styles](sources-styles/index.md) through [OGC API - Styles](https://docs.ogc.org/DRAFTS/20-009.html), each titled after its `name`.
`/ogc/styles/{styleID}` is an alias of `/style/{styleID}`.
Martin does not declare the draft's `mapbox-style` class, which names the Mapbox media type.

OGC API - Styles is still a draft, so this part may change with it.

## Maps

With [rendering](sources-styles/rendering.md) enabled, Martin also renders maps through [OGC API - Maps](https://docs.ogc.org/is/20-058/20-058.html).
`/ogc/styles/{styleID}/map` is a style rendered as one image.
`/ogc/collections/{sourceID}/map` is a raster source as one image.
Only raster sources on `WebMercatorQuad` have such a map, and their collections link it; vector data is drawn through a style.
Each collection also gives its `storageCrs`, the CRS of its grid, with its bounds in that CRS when Martin can convert them.
A map is in the format the `Accept` header weighs highest, ignoring `q=0`: PNG (also for `*/*` or no header) or JPEG.

| Parameter           | Meaning                                                                                                     |
|---------------------|-------------------------------------------------------------------------------------------------------------|
| `width`, `height`   | size in pixels, up to 2048; with one, the other follows the area; with neither, the longer side is 1024     |
| `bbox`              | area as corners in `bbox-crs` axis order; defaults to a collection's bounds, or the world for a style       |
| `bbox-crs`          | CRS of `bbox`: CRS84 (default), EPSG:4326 or EPSG:3857                                                      |
| `center`            | point as `x,y` to center the map on, instead of `bbox`                                                      |
| `center-crs`        | CRS of `center`, as for `bbox-crs`                                                                          |
| `scale-denominator` | scale of the map, instead of `width` and `height` with `bbox`                                               |

Maps are always in web mercator, which the `Content-Crs` header states.
The `Content-Bbox` header gives the area shown, in metres: when its shape differs from the image's, the map shows more than was asked for.

## Conformance

`/ogc/conformance` lists which parts of the standards Martin implements:

- OGC API - Tiles core, tileset, tilesets list, geodata tilesets and Mapbox Vector Tiles
- OGC API - Common core, landing page and JSON
- Two Dimensional Tile Matrix Set 2.0 and its JSON encoding
- OGC API - Styles core and global styles, when Martin is built with styles
- OGC API - Maps core, collection map, styled map, PNG and JPEG, when rendering is enabled

We run the official OGC test suite against Martin in CI.

## Reading Martin with GDAL

GDAL's OGC API driver can read Martin's vector tiles.
It shows each zoom level as a separate layer:

```bash
ogrinfo OGCAPI:http://localhost:3000/ogc/collections/world_cities -oo API=TILES
```
