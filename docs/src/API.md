# API

When started, Martin will go through all spatial tables and functions with an appropriate signature in the database. These tables and functions will be available as the HTTP endpoints, which you can use to query Mapbox vector tiles.

| Method | URL                                    | Description                                             |
|--------|----------------------------------------|---------------------------------------------------------|
| `GET`  | `/`                                    | Status text, that will eventually show web UI           |
| `GET`  | `/catalog`                             | [List of all sources](#source-list)                     |
| `GET`  | `/{sourceID}`                          | [Source TileJSON](#table-source-tilejson)               |
| `GET`  | `/{sourceID}/{z}/{x}/{y}`              | [Source Tiles](#table-source-tiles)                     |
| `GET`  | `/{sourceID1},...,{nameN}`             | [Composite Source TileJSON](#composite-source-tilejson) |
| `GET`  | `/{sourceID1},...,{nameN}/{z}/{x}/{y}` | [Composite Source Tiles](#composite-source-tiles)       |
| `GET`  | `/health`                              | Martin server health check: returns 200 `OK`            |
