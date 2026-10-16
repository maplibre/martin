#!/usr/bin/env python3
"""Regenerate the cog-tiles test COGs in this directory.
Requires GDAL's Python bindings and numpy.

    python3 tests/fixtures/cog/synthetic/generate.py

Every raster uses the same synthetic pattern (see `pattern`), so tests can recompute the
expected pixels. The aligned files cover z15 tile 031313131130112 (x=16374, y=10897)
at z16 resolution with 256 px blocks.
"""

import os

import numpy as np
from osgeo import gdal, osr

gdal.UseExceptions()

HERE = os.path.dirname(os.path.abspath(__file__))
HALF = 20037508.342789244
QUADKEY = "031313131130112"


def tile_bounds(quadkey):
    x = y = 0
    for c in quadkey:
        d = int(c)
        x, y = x * 2 + (d & 1), y * 2 + (d >> 1)
    size = 2 * HALF / 2 ** len(quadkey)
    return -HALF + x * size, HALF - y * size, size


def pattern(width, height, bands, shift=0):
    """Band b of pixel (x, y) is (x + 2y + 64b + shift) mod 256."""
    ys, xs = np.mgrid[0:height, 0:width]
    return np.stack([((xs + 2 * ys + 64 * b + shift) % 256).astype(np.uint8) for b in range(bands)])


def write_cog(name, data, origin, res, epsg=3857, dtype=gdal.GDT_Byte, nodata=None, **options):
    bands, height, width = data.shape
    mem = gdal.GetDriverByName("MEM").Create("", width, height, bands, dtype)
    mem.SetGeoTransform([origin[0], res, 0, origin[1], 0, -res])
    srs = osr.SpatialReference()
    srs.ImportFromEPSG(epsg)
    mem.SetProjection(srs.ExportToWkt())
    for b in range(bands):
        band = mem.GetRasterBand(b + 1)
        if nodata is not None:
            band.SetNoDataValue(nodata)
        band.WriteArray(data[b])
    if bands == 4:
        mem.GetRasterBand(4).SetColorInterpretation(gdal.GCI_AlphaBand)
    creation = ["BLOCKSIZE=256", "OVERVIEW_RESAMPLING=NEAREST"] + [
        f"{k}={v}" for k, v in options.items()
    ]
    path = os.path.join(HERE, name)
    os.makedirs(os.path.dirname(path), exist_ok=True)
    gdal.Translate(path, mem, format="COG", creationOptions=creation)
    aux = path + ".aux.xml"
    if os.path.exists(aux):
        os.remove(aux)
    print(f"{name}: {os.path.getsize(path)} bytes")


def main():
    min_x, max_y, size = tile_bounds(QUADKEY)
    res = size / 512  # z16 at 256 px tiles
    origin = (min_x, max_y)

    write_cog("rgb_jpeg.tif", pattern(512, 512, 3), origin, res, COMPRESS="JPEG", QUALITY=90)
    write_cog("rgb_webp.tif", pattern(512, 512, 3), origin, res, COMPRESS="WEBP", QUALITY=90)
    write_cog("rgba_deflate.tif", pattern(512, 512, 4), origin, res, COMPRESS="DEFLATE", PREDICTOR=2)
    write_cog("gray_lzw.tif", pattern(512, 512, 1), origin, res, COMPRESS="LZW")
    write_cog("rgb_zstd.tif", pattern(512, 512, 3), origin, res, COMPRESS="ZSTD")

    # The south-east quarter is all NoData, so its block is not written at all.
    sparse = pattern(512, 512, 3)
    sparse[:, 256:, 256:] = 0
    write_cog("sparse.tif", sparse, origin, res, nodata=0, COMPRESS="DEFLATE", SPARSE_OK="TRUE")

    # Files that must be rejected.
    write_cog(
        "float32.tif",
        pattern(512, 512, 1).astype(np.float32),
        origin,
        res,
        dtype=gdal.GDT_Float32,
        COMPRESS="DEFLATE",
    )
    write_cog("utm.tif", pattern(512, 512, 3), (699000.0, 5710000.0), 0.5, epsg=32630, COMPRESS="JPEG")
    write_cog("unaligned.tif", pattern(512, 512, 3), (min_x + res / 2, max_y), res, COMPRESS="JPEG")
    write_cog("odd_resolution.tif", pattern(512, 512, 3), origin, res * 1.1, COMPRESS="JPEG")


if __name__ == "__main__":
    main()
