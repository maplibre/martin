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
