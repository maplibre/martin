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

## Supported Projection

Currently we only support COGS with [EPSG:3857](https://epsg.io/3857) by enable the `auto-web` option.

It's beacause the [Tile Matrix Set](https://docs.ogc.org/is/17-083r2/17-083r2.html#72) inside each COG file is highly customized for its extent and tilesize. It's not aligned
with any well knowed TIle Matrix Set.

To load COG file, there are two approaches generally:

1. The client(`Maplibre`, `OpenLayers`,etc) load COG file with the specific customized [Tile Matrix Set](https://docs.ogc.org/is/17-083r2/17-083r2.html#72).
  To not break the compatibility with the [TileJson spec](https://github.com/mapbox/tilejson-spec), the `/catalog` seems a good choice to add the customized TMS info (Other data sources could benefit from this if we want to support other projections either, [Join our discussion there](https://github.com/maplibre/martin/issues/343))

2. Martin serve COG files with well known [Tile Matrix Set](https://docs.ogc.org/is/17-083r2/17-083r2.html#72) and do the clipping internally.
   Currently, we support [WebMercatorQuad](https://docs.ogc.org/is/17-083r2/17-083r2.html#72) if `auto-web: true` is configured.

## Configuration file

```yml
cog:
  # Default false
  # If enabled:
  #   Serve COG with WebMercatorQuad
  # Note: Just work for COG files with EPSG:3857
  auto_web: false
  sources:
    cog-src2: tests/fixtures/cog/rgb_u8.tif
    cog-src1: tests/fixtures/cog/rgba_u8.tif
    # Test COG with auto_webmercator enabled
    cog-auto-web:
      path: tests/fixtures/cog/rgba_u8_nodata.tiff
      # inline option. Would override the global dauto_web.
      auto_web: true
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
