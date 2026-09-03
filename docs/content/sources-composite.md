---
icon: material/layers-triple
tags:
  - tile-sources
  - composite-sources
  - configuration
---

# Composite Sources

Composite Sources allows combining multiple sources into one.
Composite Source consists of multiple sources separated by comma `{source1},...,{sourceN}`

Each source in a composite source can be accessed with its `{source_name}` as a `source-layer` property.

Composite source [TileJSON](https://github.com/mapbox/tilejson-spec) endpoint is available
at `/{source1},...,{sourceN}`, and tiles are available at `/{source1},...,{sourceN}/{z}/{x}/{y}`.

For example, composite source combining `points` and `lines` sources will be available at `/points,lines/{z}/{x}/{y}`

```bash
# TileJSON
curl localhost:3000/points,lines

# Whole world as a single tile
curl localhost:3000/points,lines/0/0/0
```

## Source Aliases

An alias is a named combination of tile sources that a client requests like a single source.
Each alias serves the listed sources exactly like the composite request `/{source1},...,{sourceN}`.

```yaml
mbtiles:
  paths:
    - /path/to/roads.mbtiles
    - /path/to/buildings.mbtiles
# Each alias can be requested like a tile source and serves the listed sources combined.
aliases:
  basemap: [roads, buildings]
```

Aliases may only reference tile sources, not other aliases.
An alias may share the name of a source it references; requests for that name then serve the alias.
This extends an existing source without changing the name a style uses:

```yaml
aliases:
  # Requests for "roads" also get the buildings.
  roads: [roads, buildings]
```

Aliases are listed in the catalog under their own name with the format of the sources they combine.
