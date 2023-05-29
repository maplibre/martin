# Composite Sources

Composite Sources allows combining multiple sources into one. Composite Source consists of multiple sources separated by comma `{source1},...,{sourceN}`

Each source in a composite source can be accessed with its `{source_name}` as a `source-layer` property.

## Composite Source TileJSON

Composite Source [TileJSON](https://github.com/mapbox/tilejson-spec) endpoint is available at `/{source1},...,{sourceN}`.

For example, composite source combining `points` and `lines` sources will be available at `/points,lines`

```shell
curl localhost:3000/points,lines | jq
```

## Composite Source Tiles

Composite Source tiles endpoint is available at `/{source1},...,{sourceN}/{z}/{x}/{y}`

For example, composite source combining `points` and `lines` sources will be available at `/points,lines/{z}/{x}/{y}`

```shell
curl localhost:3000/points,lines/0/0/0
```
