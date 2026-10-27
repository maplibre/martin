"""Regenerate the reprojection test COG in this directory and the tiles GDAL warps it to.
Requires numpy and the GDAL command line tools.

    python3 tests/fixtures/cog/warped/generate.py

`polar.tif` is a small COG in NSIDC polar stereographic north (EPSG:3413), which cannot be
reprojected. `utm.tif` is a 4-band COG in UTM zone 10N (EPSG:26910) in San Francisco, 2 m pixels, with a
triangle of nodata (0) in its top-left corner. Each `expected/{z}_{x}_{y}.png` is its first three
bands warped by `gdalwarp` onto that Web Mercator tile with nearest-neighbour resampling, from the
coarsest overview at least as detailed as the tile, with an alpha band that is transparent where
the file has no data.
"""

import math
import os
import subprocess
import tempfile

import numpy as np

HERE = os.path.dirname(os.path.abspath(__file__))
HALF = 20037508.342789244
WIDTH, HEIGHT, BANDS = 768, 640, 4
ORIGIN = (548000.0, 4184000.0)
RES = 2.0
TILES = [(13, 1309, 3165), (14, 2619, 6331), (15, 5238, 12663), (16, 10477, 25326), (17, 20954, 50652), (17, 20957, 50652), (17, 20951, 50649), (17, 20952, 50650)]


def pattern():
    """Band b of pixel (x, y) is (x + 2y + 64b) mod 255 + 1, and 0 where x + y < 200."""
    ys, xs = np.mgrid[0:HEIGHT, 0:WIDTH]
    data = np.stack([((xs + 2 * ys + 64 * b) % 255 + 1).astype(np.uint8) for b in range(BANDS)])
    data[:, xs + ys < 200] = 0
    return data


def run(*args):
    subprocess.run(args, check=True, capture_output=True)


def write_cog(path, tmp):
    raw = os.path.join(tmp, "utm.raw")
    np.ascontiguousarray(pattern().transpose(1, 2, 0)).tofile(raw)
    bands = "".join(
        f'<VRTRasterBand dataType="Byte" band="{b + 1}" subClass="VRTRawRasterBand">'
        f"<SourceFilename>{raw}</SourceFilename><ImageOffset>{b}</ImageOffset>"
        f"<PixelOffset>{BANDS}</PixelOffset><LineOffset>{WIDTH * BANDS}</LineOffset></VRTRasterBand>"
        for b in range(BANDS)
    )
    vrt = os.path.join(tmp, "utm.vrt")
    with open(vrt, "w") as f:
        f.write(f'<VRTDataset rasterXSize="{WIDTH}" rasterYSize="{HEIGHT}">{bands}</VRTDataset>')
    ulx, uly = ORIGIN
    run(
        "gdal_translate", "-of", "COG", "-a_srs", "EPSG:26910", "-a_nodata", "0",
        "-a_ullr", str(ulx), str(uly), str(ulx + WIDTH * RES), str(uly - HEIGHT * RES),
        "-co", "COMPRESS=DEFLATE", "-co", "BLOCKSIZE=256", "-co", "OVERVIEW_RESAMPLING=NEAREST",
        vrt, path,
    )


def write_polar(path, tmp):
    run(
        "gdal_create", "-of", "GTiff", "-outsize", "256", "256", "-bands", "1", "-burn", "7",
        "-a_srs", "EPSG:3413", "-a_ullr", "0", "0", "25600", "-25600", os.path.join(tmp, "polar.tif"),
    )
    run("gdal_translate", "-of", "COG", "-co", "BLOCKSIZE=256", os.path.join(tmp, "polar.tif"), path)


def overview_for(z):
    """The overview (as `gdalwarp -ovr` counts them) this crate picks for zoom z, or NONE."""
    lat = 37.8
    tile_res = 2 * HALF / 2**z / 256
    scale = tile_res * math.cos(math.radians(lat)) / RES
    factor = 1
    while factor * 2 <= scale and WIDTH / (factor * 2) >= 1 and factor * 2 <= 4:
        factor *= 2
    return "NONE" if factor == 1 else str(int(math.log2(factor)) - 1)


def warp(cog, tmp, z, x, y):
    size = 2 * HALF / 2**z
    minx, maxy = -HALF + x * size, HALF - y * size
    rgb = os.path.join(tmp, "rgb.vrt")
    run("gdal_translate", "-of", "VRT", "-b", "1", "-b", "2", "-b", "3", cog, rgb)
    warped = os.path.join(tmp, f"{z}_{x}_{y}.tif")
    run(
        "gdalwarp", "-overwrite", "-t_srs", "EPSG:3857", "-te", str(minx), str(maxy - size), str(minx + size), str(maxy),
        "-ts", "256", "256", "-r", "near", "-et", "0", "-ovr", overview_for(z), "-dstalpha", "-wo", "UNIFIED_SRC_NODATA=YES",
        rgb, warped,
    )
    os.makedirs(os.path.join(HERE, "expected"), exist_ok=True)
    run("gdal_translate", "-of", "PNG", warped, os.path.join(HERE, "expected", f"{z}_{x}_{y}.png"))


def main():
    cog = os.path.join(HERE, "utm.tif")
    with tempfile.TemporaryDirectory() as tmp:
        write_cog(cog, tmp)
        write_polar(os.path.join(HERE, "polar.tif"), tmp)
        for z, x, y in TILES:
            warp(cog, tmp, z, x, y)
    for name in os.listdir(os.path.join(HERE, "expected")):
        if name.endswith(".aux.xml"):
            os.remove(os.path.join(HERE, "expected", name))


if __name__ == "__main__":
    main()
