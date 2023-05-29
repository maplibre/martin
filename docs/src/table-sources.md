# Table Sources

Table Source is a database table which can be used to query [vector tiles](https://github.com/mapbox/vector-tile-spec). When started, Martin will go through all spatial tables in the database and build a list of table sources. A table should have at least one geometry column with non-zero SRID. All other table columns except geometry will be properties of a vector tile feature.

## Table Source TileJSON

Table Source [TileJSON](https://github.com/mapbox/tilejson-spec) endpoint is available at `/{table_name}`.

For example, `points` table will be available at `/points`, unless there is another source with the same name, or if the table has multiple geometry columns, in which case it will be available at `/points`, `/points.1`, etc.

```shell
curl localhost:3000/points | jq
```

## Table Source Tiles

Table Source tiles endpoint is available at `/{table_name}/{z}/{x}/{y}`

For example, `points` table will be available at `/points/{z}/{x}/{y}`

```shell
curl localhost:3000/points/0/0/0
```

In case if you have multiple geometry columns in that table and want to access a particular geometry column in vector tile, you should also specify the geometry column in the table source name

```shell
curl localhost:3000/points.geom/0/0/0
```
