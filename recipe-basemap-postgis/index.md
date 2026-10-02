# Setting up a Basemap and Overlaying Points from PostGIS

You commonly have some semi-proprietary datasource which you want to overlay onto another. This guide shows how to generate a basemap using [Planetiler](<https://github.com/onthegomap/planetiler/>) from [OSM](<https://osm.org>) and overlay custom points from a [PostGIS](<https://postgis.net/>) database. See here for a discussion on the [pros and cons of this/alternative data sources](<https://maplibre.org/martin/sources-tiles/index.md>).

## Prerequisites

We expect you have the following already installed:

- [Docker](<https://docker.io>)
- [Martin binary](<https://maplibre.org/martin/installation/index.md>)

## Generate an MBTiles basemap with Planetiler

There are multiple ways to generate a tile archive. For semi-static tile archives, we think using [Planetiler](<https://github.com/onthegomap/planetiler/>) to build MBtiles archives using [OpenMapTiles](<https://openmaptiles.org/>) is a good starting point.

**🤔 *Why do I need a tool to convert OSM to mbtiles in the first place?* (click to expand)**

The reason you need a tool to build vector tilesets from OpenStreetMap is that the data in OpenStreetMap is - not following a specific schema, - nor pre-tiled into \`x\`/\`y\`/\`z\` chunks.

**🤔 *What is up with MBtiles and OpenMapTiles* (click to expand)**

Good question. MBtiles is the archive format. Think of a sqlite database storing the data you need a chunk (\`x\`/\`y\`/\`z\`) of the world. See our comparison \[pmtiles vs. mbtiles\](sources-tiles/index.md) for discussions on the pros and cons of this/alternative formats. But how does the data in the archive look like? This is where the vector tile schema comes in: \[OpenMapTiles\](https://openmaptiles.org/) defines which layers are included in the served data and how they are aggregated. \[OpenMapTiles\](https://openmaptiles.org/) does have an attribution requirement. You will need to have \[\`© OpenMapTiles\`\](https://openmaptiles.org/) at the bottom of your map. See \[Shortbread\](https://shortbread-tiles.org/) for a newer, but less mature alternative if you want to read more.

Below command downloads a tile archive at `data/monaco.mbtiles` for monaco.

Please refer to [Planetilers documentation](<https://github.com/onthegomap/planetiler/>) on different download options.

```bash
mkdir --parents data
docker run \
  --user=$UID \
  -e JAVA_TOOL_OPTIONS="-Xmx1g" \
  -v "$(pwd)/data":/data \
  --rm \
  ghcr.io/onthegomap/planetiler:latest \
  --download \
  --minzoom=0 \
  --maxzoom=14 \
  --tile_compression=none \
  --area=monaco \
  --output /data/monaco.mbtiles
```

## Loading data into a PostGIS database

### Run PostGIS

First, you need a running [postGIS](<https://postgis.net>) instance.

```bash
docker run \
  --name some-postgis \
  --env POSTGRES_PASSWORD=mypass \
  --publish 5432:5432 \
  --detach \
  postgis/postgis
```

### Import Points into PostGIS

Then you need to add geometries to the PostGIS database. This is possible via multiple ways, such as [`osm2pgsql` for adding specific, updatable OSM data](<https://osm2pgsql.org/>) or in the programming language of your choice via postgres' various database connectors.

```bash
docker exec some-postgis psql --dbname postgres --username postgres --command \
  "CREATE TABLE where_yachts_can_be_looked_at ("\
  "  title TEXT NOT NULL, "\
  "  subtitle TEXT NOT NULL, "\
  "  location GEOMETRY(Point, 4326) NOT NULL);"

docker exec some-postgis psql --dbname postgres --username postgres --command \
  "INSERT INTO where_yachts_can_be_looked_at (title, subtitle, location) VALUES ( "\
  "  'Port Hercules', "\
  "  'Great view of superyachts docked in the iconic harbor.', "\
  "  ST_SetSRID(ST_MakePoint(7.424789, 43.735217), 4326));"
```

## Serving tiles with Martin

Now we will serve the content of [mbtiles](<https://maplibre.org/martin/sources-mbtiles/index.md>) and the [postgis database](<https://maplibre.org/martin/pg-connections/index.md>).

If you want more precise options what things are published how, please see the [configuration file](<https://maplibre.org/martin/config-file/index.md>) or [cli documentation](<https://maplibre.org/martin/run-with-cli/index.md>). By default, we will share every servable postgres [table, view](<https://maplibre.org/martin/sources-pg-tables/index.md>) and [function](<https://maplibre.org/martin/sources-pg-functions/index.md>).

```bash
martin data/monaco.mbtiles postgres://postgres:mypass@localhost:5432/postgres
```

You can now see the catalog of available things here: [`http://localhost:3000/catalog`](<http://localhost:3000/catalog>) The tilejson endpoint of both tilesets is [`http://localhost:3000/monaco,where_yachts_can_be_looked_at`](<http://localhost:3000/monaco,where_yachts_can_be_looked_at>)

## Using in Maputnik to style a map

Due to CORS, we CANNOT use the website [https://maplibre.org/maputnik/](<https://maplibre.org/maputnik/>) without further setup. Once the tiles are deployed behind https, this can be used.

To get a local version of maputnik, run

```bash
docker run -it --rm -p 8888:80 ghcr.io/maplibre/maputnik:main
```

Maputnik is now online, so lets load martins tiles into it.

1. Visit [`http://localhost:8888`](<http://localhost:8888>)
2. You first need a style:
3. Click `Open`
4. Select a style you like (we are going to choose `Maptiler Basic`)
5. You now have a style using Maptilers' (**NOT martins**) data. You need to change its datasource to use the tiles we just published:
6. Click `Data Sources`
7. And add the tilejson from above `http://localhost:3000/monaco,where_yachts_can_be_looked_at`:
8. Now, lets zoom into monaco:
9. And finally, lets add a circle layer for our yachts:
10. Click `Add Layer`
11. Configure the layer as follows:
12. And style it as:

    **json configuration (click to expand)**

    ```json
    {
      "id": "where_yachts_can_be_looked_at",
      "type": "circle",
      "source": "openmaptiles",
      "source-layer": "where_yachts_can_be_looked_at",
      "paint": {
        "circle-color": "rgba(255, 7, 103, 1)",
        "circle-blur": 0.2,
        "circle-radius": {
          "stops": [
            [13, 1],
            [15, 300],
            [20, 2000]
          ]
        },
        "circle-opacity": {
          "stops": [[13.5, 0], [14, 0.4]]
        }
      },
      "minzoom": 13
    }
    ```
