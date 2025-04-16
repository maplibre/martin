## Quick start on Linux

```bash
mkdir martin
cd martin

# Download some sample data
curl -L -O https://github.com/maplibre/martin/raw/main/tests/fixtures/mbtiles/world_cities.mbtiles

# Download the latest version of Martin binary, extract it, and make it executable
curl -L -O https://github.com/maplibre/martin/releases/latest/download/martin-x86_64-unknown-linux-gnu.tar.gz
tar -xzf martin-x86_64-unknown-linux-gnu.tar.gz
chmod +x ./martin

# Show Martin help screen
./martin --help

# Run Martin with the sample data as the only tile source
./martin world_cities.mbtiles
```

### View the map

See [quick start with QGIS](qgis.md) for instructions on how to view the map.
