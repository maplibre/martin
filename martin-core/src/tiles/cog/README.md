## COG Image Representation

* COG file is an image container representing a tile grid
* A COG may have multiple images, also called subfiles or overviews, each indexed with an Image File Directory number - [`IFD`](https://download.osgeo.org/libtiff/doc/TIFF6.pdf#[{"num":209,"gen":0},{"name":"FitB"}]#[{"num":76,"gen":0},{"name":"FitB"}]#[{"num":76,"gen":0},{"name":"FitB"}]])
* A COG must have at least one image.
* The first image (IFD=0) must be a full resolution image, e.g., the one with the highest resolution.
* [Each image may also have an accompanying mask](https://docs.ogc.org/is/21-026/21-026.html#_requirement_reduced_resolution_subfiles), which is also indexed with an IFD. The mask is used to [define a transparency mask](https://www.verypdf.com/document/tiff6/pg_0036.htm). We do not support masks yet.
* While uncommon, COG tile matrix set ([2D TMS](https://docs.ogc.org/is/17-083r4/17-083r4.html#tilematrixset-requirements-class)) might be different from the common WebMercatorQuad. We do not support any TMS other than Web Mercator yet.

### COG IFD Structure

Here is an example of a tile grid for a COG file with five images.

| ifd | image index | resolution | zoom |
| --- | ----------- | ---------- | ---- |
| 0   | 0           | 20         | 4    |
| 1   | 1           | 40         | 3    |
| 2   | 2           | 80         | 2    |
| 3   | 3           | 160        | 1    |
| 4   | 4           | 320        | 0    |

### COG file requirements enforced by Martin

Due to the flexibility of the COG, GEOTIFF and TIFF file formats and the assumptions of Martin, not all COG files will be compatible. The following are a few requirements that Martin has of the COG file, some of which are defined in the COG or TIFF spec, some are constraints imposed by assumptions of Martin. If your file conforms to these requirements, it's more likely to work with Martin:

* File MUST define the ProjectedCRS GeoKey with a value of 3857 (EPSG:3857)
* File MUST use PlanarConfiguration=1 aka. "Contiguous" or "Chunky"
* File MUST use Compression=1 "None", 5 "LZW" or 8 "Deflate" [See GDAL comrpession tag definitions](https://github.com/OSGeo/gdal/blob/c7d41bf263a1a3951546c5cfa66872fc05dfc8cc/frmts/gtiff/libtiff/tiff.h#L182-L219)
* File MUST use an 8-bit color depth
* File MUST use 3 (RGB) or 4 (RGBA) bands
* File MUST use tile blocks not strips (eg. TileWidth, TileHeight is defined, not StripOffsets, StripByteCounts, RowsPerStrip, etc.)
* File MUST use square tiles whose dimension is a power of 2 (eg. 256x256 or 512x512)

Using GDAL, you can create a COG file which meets most of these requirements using:

```
gdal_translate original.tif compatible.tif -b 1 -b 2 -b 3 -of COG -co BIGTIFF=YES -co TILING_SCHEME=GoogleMapsCompatible -co ADD_ALPHA=YES -co OVERVIEWS=IGNORE_EXISTING -co COMPRESS=LZW -co OVERVIEW_COUNT=4 -co ALIGNED_LEVELS=5 -co NUM_THREADS=ALL_CPUS -co ZOOM_LEVEL_STRATEGY=LOWER -co BLOCKSIZE=512
```
