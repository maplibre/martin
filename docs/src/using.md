## Martin Endpoints

Martin data is available via the HTTP `GET` endpoints:

| URL                                     | Description                                    |
|-----------------------------------------|------------------------------------------------|
| `/`                                     | Status text, that will eventually show web UI  |
| `/catalog`                              | [List of all sources](#catalog)                |
| `/{sourceID}`                           | [Source TileJSON](#source-tilejson)            |
| `/{sourceID}/{z}/{x}/{y}`               | Map Tiles                                      |
| `/{source1},…,{sourceN}`                | [Composite Source TileJSON](#source-tilejson)  |
| `/{source1},…,{sourceN}/{z}/{x}/{y}`    | [Composite Source Tiles](sources-composite.md) |
| `/sprite/{spriteID}[@2x].{json,png}`    | [Sprite sources](sources-sprites.md)           |
| `/font/{font}/{start}-{end}`            | [Font source](sources-fonts.md)                |
| `/font/{font1},…,{fontN}/{start}-{end}` | [Composite Font source](sources-fonts.md)      |
| `/health`                               | Martin server health check: returns 200 `OK`   |
| `POST /refresh`                         | Refresh MBTiles/PMTiles/PostGIS sources        |

### Duplicate Source ID

In case there is more than one source that has the same name, e.g. a PG function is available in two
schemas/connections, or a table has more than one geometry columns, sources will be assigned unique IDs such
as `/points`, `/points.1`, etc.

### Reserved Source IDs

Some source IDs are reserved for internal use. If you try to use them, they will be automatically renamed to a unique ID
the same way as duplicate source IDs are handled, e.g. a `catalog` source will become `catalog.1`.

Some of the reserved IDs: `_`, `catalog`, `config`, `font`, `health`, `help`, `index`, `manifest`, `metrics`, `refresh`,
`reload`, `sprite`, `status`.

### Catalog

A list of all available sources is available via catalogue endpoint:

```bash
curl localhost:3000/catalog | jq
```

```yaml
{
  "tiles" {
  "function_zxy_query": {
    "name": "public.function_zxy_query",
    "content_type": "application/x-protobuf"
  },
  "points1": {
    "name": "public.points1.geom",
    "content_type": "image/webp"
  },
  ...
},
  "sprites": {
    "cool_icons": {
      "images": [
        "bicycle",
        "bear",
      ]
    },
               ...
  },
  "fonts": {
    "Noto Mono Regular": {
      "family": "Noto Mono",
      "style": "Regular",
      "glyphs": 875,
      "start": 0,
      "end": 65533
    },
             ...
  }
}
```

### Source TileJSON

All tile sources have a [TileJSON](https://github.com/mapbox/tilejson-spec) endpoint available at the `/{SourceID}`.

For example, a `points` function or a table will be available as `/points`. Composite source combining `points`
and `lines` sources will be available at `/points,lines` endpoint.

```bash
curl localhost:3000/points | jq
curl localhost:3000/points,lines | jq
```
