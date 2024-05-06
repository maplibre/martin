## With Windows

### Download Martin And Data

1. Download the demo `world_cities.mbtiles
` , file from the [martin repo](https://github.com/maplibre/martin/blob/main/tests/fixtures/mbtiles/world_cities.mbtiles).

2. Download the latest version of Martin binary from the [release page](https://github.com/maplibre/martin/releases), for windows it's the `martin-x86_64-pc-windows-msvc.zip
` file.

3. Extract `martin-x86_64-pc-windows-msvc.zip`, and place the `martin.exe` and the `world_cities.mbtiles` in the same directory.

### Run Martin

1. Open the command prompt and navigate to the directory where `martin.exe` and `world_cities.mbtiles` are located.

2. Run the following command to start Martin with the demo data:

```shell
martin.exe --help
martin.exe .\world_citie.mbtiles
```

### View The Map

See [View The Map](quick-start-view-map.md) for instructions on how to view the map with QGIS.
