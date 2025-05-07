# Cloud Optimized GeoTIFF File Sources

Martin can also serve raster sources like local [COG(Cloud Optimized GeoTIFF)](https://cogeo.org/) files. For cog on remote like S3 and other improvements, you could track them on [issue 875](https://github.com/maplibre/martin/issues/875), we are working on and welcome any assistance.

## Supported colortype and bits per sample

| colory type | bits per sample | supported | status     |
| ----------- | --------------- | --------- | ---------- |
| rgb/rgba    | 8               | ✅         |            |
| rgb/rgba    | 16/32...        | 🛠️         | working on |
| gray        | 8/16/32...      | 🛠️         | working on |

## Supported compression

* None
* LZW
* Deflate
* PackBits

## Run Martin with CLI to serve cog files

```bash
# Configured with a directory containing `*.tif` or `*.tiff` TIFF files.
martin /with/tiff/dir1 /with/tiff/dir2
# Configured with dedicated TIFF file
martin /path/to/target1.tif /path/to/target2.tiff
# Configured with a combination of directories and dedicated TIFF files.
martin /with/tiff/files /path/to/target1.tif /path/to/target2.tiff
```

## Run Martin with configuration file

```yml
keep_alive: 75

# The socket address to bind [default: 0.0.0.0:3000]
listen_addresses: '0.0.0.0:3000'

# Number of web server workers
worker_processes: 8

# Amount of memory (in MB) to use for caching tiles [default: 512, 0 to disable]
cache_size_mb: 8

# Database configuration. This can also be a list of PG configs.

cog:
  paths:
    # scan this whole dir, matching all *.tif and *.tiff files
    - /dir-path
    # specific TIFF file will be published as a cog source
    - /path/to/target1.tif
    - /path/to/target2.tiff
  sources:
    # named source matching source name to a single file
     cog-src1: /path/to/cog1.tif
     cog-src2: /path/to/cog2.tif
```

## Tile Grid

Generally `COG` file has a custom tile grid which is not aligned to the google 3857 which is default in almost any map client like MapLibre, OpenLayers, etc..

To display the `COG` file the clients needs to know the custom `tile grid` of COG.

To not break the compatiblity of TileJSON spec, martin choose to add a field in `TileJSON` to tell the custom tile grid.

Lile we have a cog source named `rgb_u8`, we could see the custom filed in our `TileJson` by visit `http://your_host:your_port/rgb_u8`.

```json
{
  "maxzoom": 3,
  "minzoom": 0,
  "tilejson": "3.0.0",
  "tiles": [
    "http://localhost:3111/rgb_u8/{z}/{x}/{y}"
  ],
  "custom_grid": {   // the custom tile grid added here
    "extent": [
      1620750.2508,
      4271892.7153,
      1625870.2508,
      4277012.7153
    ],
    "maxZoom": 3,
    "minZoom": 0,
    "origin": [
      1620750.2508,
      4277012.7153
    ],
    "resolutions": [
      80,
      40,
      20,
      10
    ],
    "tileSize": [
      256,
      256
    ]
  },
}
```

A demo about how to load it with `openlayers`.

```js
import './style.css';
import { Map, View } from 'ol';
import TileLayer from 'ol/layer/Tile';
import OSM from 'ol/source/OSM';
import XYZ from 'ol/source/XYZ.js';


import TileGrid from 'ol/tilegrid/TileGrid.js';

var custom_grid = new TileGrid({
  extent: [
    1620750.2508,
    4271892.7153,
    1625870.2508,
    4277012.7153],
  resolutions: [
    80,
    40,
    20,
    10],
  tileSize: [256, 256],
  origin: [1620750.2508, 4277012.7153]
});

var source = new XYZ({
  url: "http://10.1.155.35:3000/rgb_u8/{z}/{x}/{y}", tileGrid: custom_grid
});

const map = new Map({
  target: 'map',
  layers: [
    new TileLayer({
      source: source
    }),
  ],
  view: new View({
    center: [(1620750.2508 + 1625870.2508) / 2, (4271892.7153 + 4277012.7153) / 2],
    zoom: 14
  })
});
```

## About COG

[COG](https://cogeo.org/) is just Cloud Optimized GeoTIFF file.

TIFF is an image file format. TIFF tags are something like key-value pairs inside to describe the metadata about a TIFF file, ike `ImageWidth`, `ImageLength`, etc.

GeoTIFF is a valid TIFF file with a set of TIFF tags to describe the 'Cartographic' information associated with it.

COG is a valid GeoTIFF file with some requirements for efficient reading. That is, all COG files are valid GeoTIFF files, but not all GeoTIFF files are valid COG files. For quick access to tiles in TIFF files, Martin relies on the requirements/recommendations(like the [requirement about Reduced-Resolution Subfiles](https://docs.ogc.org/is/21-026/21-026.html#_requirement_reduced_resolution_subfiles) and [the content dividing strategy](https://docs.ogc.org/is/21-026/21-026.html#_tiles)) so we use the term `COG` over `GeoTIFF` in our documentation and configuration files.

You may want to visit these specs:

* [TIFF 6.0](https://www.itu.int/itudoc/itu-t/com16/tiff-fx/docs/tiff6.pdf)
* [GeoTIFF](https://docs.ogc.org/is/19-008r4/19-008r4.html)
* [Cloud Optimized GeoTIFF](https://docs.ogc.org/is/21-026/21-026.html)

### COG generation with GDAL

You could generate cog with `gdal_translate` or `gdalwarp`. See more details in [gdal doc](https://gdal.org/en/latest/drivers/raster/cog.html).

```bash
# gdal-bin installation
# sudo apt update
# sudo apt install gdal-bin

# gdalwarp
gdalwarp src1.tif src2.tif out.tif -of COG

# or gdal_translate
gdal_translate input.tif output_cog.tif -of COG
```

### The mapping from ZXY to tiff chunk

* A single TIFF file could contains many sub-file about same spatial area, each has different resolution
* A sub file is organized with many tiles

So basically there's a mapping from zxy to tile of sub-file of TIFF.

| zxy        | mapping to                  |
| ---------- | --------------------------- |
| Zoom level | which sub-file in TIFF file |
| X and Y    | which tile in subfile       |

Clients could read only the header part of COG to figure out the mapping from zxy to the chunk number and the subfile number. Martin get tile to frontend by this mapping.
