## COG Image Representation

* COG file is an image container representing a tile grid
* A COG may have multiple images, also called subfiles or overviews, each indexed with an Image File Directory number - [`IFD`](https://download.osgeo.org/libtiff/doc/TIFF6.pdf#[{"num":209,"gen":0},{"name":"FitB"}]#[{"num":76,"gen":0},{"name":"FitB"}]#[{"num":76,"gen":0},{"name":"FitB"}]])
* A COG must have at least one image.
* The first image (IFD=0) must be a full resolution image, e.g., the one with the highest resolution.
* [Each image may also have an accompanying mask](https://docs.ogc.org/is/21-026/21-026.html#_requirement_reduced_resolution_subfiles), which is also indexed with an IFD.
  The mask is used to [define a transparency mask](https://www.verypdf.com/document/tiff6/pg_0036.htm). We do not support masks yet.
* While uncommon, COG tile matrix set ([2D TMS](https://docs.ogc.org/is/17-083r4/17-083r4.html#tilematrixset-requirements-class)) might be different from the common `WebMercatorQuad`. We do not support any TMS other than Web Mercator yet.

### COG IFD Structure

Here is an example of a tile grid for a COG file with five images.
See [wiki.openstreetmap.org](https://wiki.openstreetmap.org/wiki/Slippy_map_tilenames#Resolution_and_Scale) for more information on resolution.

| ifd | `tile_size`  | zoom | resolution (meters / px) |
| --- | ---------- | ---- | ------------------------ |
| 0   | 256        | 4    | 9783.94                  |
| 1   | 256        | 3    | 19567.88                 |
| 2   | 256        | 2    | 39135.76                 |
| 3   | 256        | 1    | 78271.52                 |
| 4   | 256        | 0    | 156543.03                |

### Resolution Error Tolerance

When matching a COG image's resolution to a WebMercatorQuad zoom level, Martin applies
`min(3.0m, resolution × 0.1%)` as the allowed error. The absolute cap of 3m dominates at low
zoom levels (z0–z5 for 256px tiles, z0–z4 for 512px tiles); the 0.1% relative threshold takes
over above that, keeping tolerance proportional to the pixel size.

| Zoom | Resolution 256px (m/px) | Tolerance 256px (m) | Resolution 512px (m/px) | Tolerance 512px (m) |
|------|-------------------------|---------------------|-------------------------|---------------------|
| 0    | 156,543.034             | 3.0 (abs cap)       | 78,271.517              | 3.0 (abs cap)       |
| 1    | 78,271.517              | 3.0 (abs cap)       | 39,135.758              | 3.0 (abs cap)       |
| 2    | 39,135.758              | 3.0 (abs cap)       | 19,567.879              | 3.0 (abs cap)       |
| 3    | 19,567.879              | 3.0 (abs cap)       | 9,783.940               | 3.0 (abs cap)       |
| 4    | 9,783.940               | 3.0 (abs cap)       | 4,891.970               | 3.0 (abs cap)       |
| 5    | 4,891.970               | 3.0 (abs cap)       | 2,445.985               | 2.445985            |
| 6    | 2,445.985               | 2.445985            | 1,222.992               | 1.222992            |
| 7    | 1,222.992               | 1.222992            | 611.496                 | 0.611496            |
| 8    | 611.496                 | 0.611496            | 305.748                 | 0.305748            |
| 9    | 305.748                 | 0.305748            | 152.874                 | 0.152874            |
| 10   | 152.874                 | 0.152874            | 76.437                  | 0.076437            |
| 11   | 76.437                  | 0.076437            | 38.219                  | 0.038219            |
| 12   | 38.219                  | 0.038219            | 19.109                  | 0.019109            |
| 13   | 19.109                  | 0.019109            | 9.555                   | 0.009555            |
| 14   | 9.555                   | 0.009555            | 4.777                   | 0.004777            |
| 15   | 4.777                   | 0.004777            | 2.389                   | 0.002389            |
| 16   | 2.389                   | 0.002389            | 1.194                   | 0.001194            |
| 17   | 1.194                   | 0.001194            | 0.597164                | 5.97e-04            |
| 18   | 0.597164                | 5.97e-04            | 0.298582                | 2.99e-04            |
| 19   | 0.298582                | 2.99e-04            | 0.149291                | 1.49e-04            |
| 20   | 0.149291                | 1.49e-04            | 0.074646                | 7.46e-05            |
| 21   | 0.074646                | 7.46e-05            | 0.037323                | 3.73e-05            |
| 22   | 0.037323                | 3.73e-05            | 0.018661                | 1.87e-05            |
| 23   | 0.018661                | 1.87e-05            | 0.009331                | 9.33e-06            |
| 24   | 0.009331                | 9.33e-06            | 0.004665                | 4.67e-06            |
| 25   | 0.004665                | 4.67e-06            | 0.002333                | 2.33e-06            |
| 26   | 0.002333                | 2.33e-06            | 0.001166                | 1.17e-06            |
| 27   | 0.001166                | 1.17e-06            | 0.000583                | 5.83e-07            |
| 28   | 0.000583                | 5.83e-07            | 0.000292                | 2.92e-07            |
| 29   | 0.000292                | 2.92e-07            | 0.000146                | 1.46e-07            |
| 30   | 0.000146                | 1.46e-07            | 0.000073                | 7.29e-08            |

### COG file requirements enforced by Martin

Due to the flexibility of the COG, GEOTIFF and TIFF file formats and the assumptions of Martin, not all COG files will be compatible.
The following are a few requirements that Martin has of the COG file, some of which are defined in the COG or TIFF spec, some are constraints imposed by assumptions of Martin.
If your file conforms to these requirements, it's more likely to work with Martin:

* File MUST define the `ProjectedCRS` `GeoKey` with a value of 3857 (EPSG:3857)
* File MUST use PlanarConfiguration=1 aka. "Contiguous" or "Chunky"
* File MUST use Compression=1 "None", 5 "LZW", 7 "`ModernJPEG`", 8 "Deflate" or 50001 "WEBP" [See GDAL compression tag definitions](https://github.com/OSGeo/gdal/blob/c7d41bf263a1a3951546c5cfa66872fc05dfc8cc/frmts/gtiff/libtiff/tiff.h#L182-L219)
* File MUST use an 8-bit color depth
* File MUST use 3 (RGB) or 4 (RGBA) bands
* File MUST use tile blocks not strips (eg. `TileWidth`, `TileHeight` is defined, not `StripOffsets`, `StripByteCounts`, `RowsPerStrip`, etc.)
* File MUST use square tiles whose dimension is a power of 2 (eg. 256x256 or 512x512)

Using GDAL, you can create a COG file with 5 zoom levels which meets most of these requirements using:

```bash
gdal_translate original.tif compatible.tif -b 1 -b 2 -b 3 -of COG -co BIGTIFF=YES -co TILING_SCHEME=GoogleMapsCompatible -co ADD_ALPHA=YES -co OVERVIEWS=IGNORE_EXISTING -co COMPRESS=LZW -co OVERVIEW_COUNT=4 -co ALIGNED_LEVELS=5 -co NUM_THREADS=ALL_CPUS -co ZOOM_LEVEL_STRATEGY=LOWER -co BLOCKSIZE=512 -co SPARSE_OK=TRUE
```
