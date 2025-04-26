# Setting up a Basemap and Overlaying Points from PostGIS

You commonly have some semi-proprietary datasource which you want to overlay onto another.
This guide shows how to generate a basemap using [Planetiler](https://github.com/onthegomap/planetiler/) from [OSM](https://osm.org) and overlay custom points from a [PostGIS](https://postgis.net/) database.

## Prerequisites

We expect you have the following already installed:
- [Docker](https://docker.io)
- [Martin binary](installation.md)

## Generate an MBTiles basemap with Planetiler

There are multiple ways to generate a tile archive.
For semi-static tile archives, we think using [Planetiler](https://github.com/onthegomap/planetiler/) to build MBtiles archives using  [OpenMapTiles](https://openmaptiles.org/) is a good starting point.

<details><summary>🤔 <i>Why do I need a tool to convert OSM to mbtiles in the first place?</i> (click to expand)</summary>

The reason you need a tool to build vector tilesets from OpenStreetMap is that the data in OpenStreetMap is

- not following a specific schema,
- nor pre-tiled into `x`/`y`/`z` chunks.

</details>

<details><summary>🤔 <i>What is up with MBtiles and OpenMapTiles</i> (click to expand)</summary>

Good question.

MBtiles is the archive format. Think of a sqlite database storing the data you need a chunk (`x`/`y`/`z`) of the world.
See our comparison [pmtiles vs. mbtiles](sources-tiles.md) for discussions on the pros and cons of this/alternative formats.

But how does the data in the archive look like?
This is where the vector tile schema comes in:
[OpenMapTiles](https://openmaptiles.org/) defines which layers are included in the served data and how they are aggregated.
[OpenMapTiles](https://openmaptiles.org/) does have an attribution requirement. You will need to add `© MapTiler` at the bottom of your map.

See [Shortbread](https://shortbread-tiles.org/) for a newer, but less mature alternative if you want to read more.

</details>

Below command downloads and a tile archive at `data/monaco.mbtiles` for monaco.
Please refer to [Planetilers documentation](https://github.com/onthegomap/planetiler/) on different download options.

```bash
mkdir --parents data
docker run --user=$UID -e JAVA_TOOL_OPTIONS="-Xmx1g" -v "$(pwd)/data":/data --interactive --tty --rm ghcr.io/onthegomap/planetiler:latest --download --minzoom=0 --maxzoom=14 --area=monaco --output monaco.mbtiles
```

## Loading data into a PostGIS database

### Run PostGIS
TODO: command to run Postgis in docker
### Import Points into PostGIS
TODO: sql insert statements in psql

## Serving tiles with Martin
TODO: command to run, available endpoints

## Using in Maputnik to style a map

TODO: Open https://maplibre.org/maputnik/
Select style
underpin with screenshots or a gifcap
