## Quick start on Windows

1. Download some [demo tiles](https://github.com/maplibre/martin/blob/main/tests/fixtures/mbtiles/world_cities.mbtiles).

2. Download the latest Windows version of Martin from
   the [release page](https://github.com/maplibre/martin/releases):  [martin-x86_64-pc-windows-msvc.zip](https://github.com/maplibre/martin/releases/latest/download/martin-x86_64-pc-windows-msvc.zip)

3. Extract content of both files and place them in a same directory.

4. Open the command prompt and navigate to the directory where `martin` and `world_cities.mbtiles` are located.

5. Run the following command to start Martin with the demo data:

```bash
# Show Martin help screen
martin --help

# Run Martin with the sample data as the only tile source
martin world_cities.mbtiles
```

### View the map

See [quick start with QGIS](quick-start-qgis.md) for instructions on how to view the map.
