# API

Martin data is available via the HTTP `GET` endpoints:

| URL                                    | Description                                                                                               |
|----------------------------------------|-----------------------------------------------------------------------------------------------------------|
| `/`                                    | Status text, that will eventually show web UI                                                             |
| `/catalog`                             | [List of all sources](https://maplibre.org/martin/source-list.html)                                       |
| `/{sourceID}`                          | [Source TileJSON](https://maplibre.org/martin/table-sources.html#table-source-tilejson)                   |
| `/{sourceID}/{z}/{x}/{y}`              | [Source Tiles](https://maplibre.org/martin/table-sources.html#table-source-tiles)                         |
| `/{sourceID1},...,{nameN}`             | [Composite Source TileJSON](https://maplibre.org/martin/composite-sources.html#composite-source-tilejson) |
| `/{sourceID1},...,{nameN}/{z}/{x}/{y}` | [Composite Source Tiles](https://maplibre.org/martin/composite-sources.html#composite-source-tiles)       |
| `/health`                              | Martin server health check: returns 200 `OK`                                                              |

## Catalog

A list of all available sources is available via catalogue endpoint:

```shell
curl localhost:3000/catalog | jq
```

```yaml
[
  {
    "id": "function_zxy_query",
    "name": "public.function_zxy_query"
  },
  {
    "id": "points1",
    "name": "public.points1.geom"
  },
  ...
]
```
