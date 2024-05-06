## With Linux

### Download Martin And Data

```shell
cd ~ && mkdir martin_demo && cd martin_demo
curl -O https://github.com/maplibre/martin/releases/download/v0.13.0/martin-x86_64-unknown-linux-gnu.tar.gz 
curl -O https://github.com/maplibre/martin/blob/main/tests/fixtures/mbtiles/world_cities.mbtiles
tar -xzf martin-x86_64-unknown-linux-gnu.tar.gz
chmod +x ./martin
```

### Run Marin With Demo MBTiles

```shell
martin --version
martin ./world_cities.mbtiles
```

### View The Map

See [View The Map](quick-start-view-map.md) for instructions on how to view the map with QGIS.
