# Cloud Optimized GeoTIFF File Sources

Martin can also serve raster source like local [COG(Cloud Optimized GeoTIFF)](https://cogeo.org/) files. For cog on remote like S3 and other improvements, you could track them on [issue 875](https://github.com/maplibre/martin/issues/875), we are working on and welcome any assistance.

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
# Configured with a directory containing TIFF files.
martin /with/tiff/dir1 /with/tiff/dir2
# Configured with dedicated TIFF file.
martin /path/to/target1.tif /path/to/target1.tif
# Configured with a combination of directories and dedicated TIFF files.
martin /with/tiff/files /path/to/target.tif
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
    # scan this whole dir, matching all *.tif files
    - /dir-path
    # specific tif file will be published as a cog source
    - /path/to/target1.tif
    - /path/to/target2.tif
  sources:
    # named source matching source name to a single file
     cog-src1: /path/to/cog1.tif
     cog-src2: /path/to/cog2.tif
```

## About COG

[COG](https://cogeo.org/) is just Cloud Optimized GeoTIFF file. You could generate cog with `gdal_translate` or `gdalwarp`. See more details in [gdal doc](https://gdal.org/en/latest/drivers/raster/cog.html).

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

* A single tif file could contains many subfile about same spatial area, each has different resollution
* A sub file is organized with many tiles

So basically there's a mapping from zxy to tile of subfile of tif.

| zxy        | mapping to                |
| ---------- | ------------------------- |
| Zoom level | which subfile in tif file |
| X and Y    | which tile in subfile     |

Clients could read only the header part of cog to figure out the mapping from zxy to the chunk number and the subfile number. And Martin get tile to frontend by this mapping.
