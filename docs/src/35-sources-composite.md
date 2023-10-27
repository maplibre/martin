# Composite Sources

Composite Sources allows combining multiple sources into one. Composite Source consists of multiple sources separated by comma `{source1},...,{sourceN}`

Each source in a composite source can be accessed with its `{source_name}` as a `source-layer` property.

Composite source [TileJSON](https://github.com/mapbox/tilejson-spec) endpoint is available at `/{source1},...,{sourceN}`, and tiles are available at `/{source1},...,{sourceN}/{z}/{x}/{y}`.

For example, composite source combining `points` and `lines` sources will be available at `/points,lines/{z}/{x}/{y}`

```shell
# TileJSON
curl localhost:3000/points,lines

# Whole world as a single tile
curl localhost:3000/points,lines/0/0/0
```
