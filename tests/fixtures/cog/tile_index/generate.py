"""Write a tile index over the synthetic NAIP COGs of `../stac`, and the tile it paints.
Requires the GDAL command line tools.

    python3 tests/fixtures/cog/tile_index/generate.py

`naip.gpkg` is what `gdaltindex -f GPKG -t_srs EPSG:4326` makes of the eight files around San
Francisco, the 2022 files first so they are painted first. It has two layers listing the same files:
`local`, by paths relative to this directory, and `s3`, by `/vsis3/naip-analytic/{key}` paths for the
e2e test to read from a `RustFS` bucket filled with `../stac/naip-analytic`. A third layer, `utm`, has
the footprints of `local` in UTM zone 10N, as `gdaltindex` writes them without `-t_srs`.

`expected/14_2619_6334.png` is what `gdalwarp` makes of the files, painted last to first so the first
wins, onto Web Mercator tile 14/2619/6334: their first three bands, nearest-neighbour, with 0 in all
three as nodata and an alpha band that is transparent where no file has data.
"""

import os
import subprocess
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
INDEX = os.path.join(HERE, "naip.gpkg")
HALF = 20037508.342789244
TILE = (14, 2619, 6334)
KEYS = [
    f"ca/{year}/60cm/rgbir_cog/37122/m_{quad}_10_060_{date}.tif"
    for year, date in [("2022", "20220518"), ("2020", "20200524")]
    for quad in ["3712213_se", "3712213_sw", "3712221_ne", "3712221_nw"]
]
LOCAL = [os.path.join("..", "stac", "naip-analytic", key) for key in KEYS]


def run(*args):
    subprocess.run(args, check=True, cwd=HERE)


def write_index():
    if os.path.exists(INDEX):
        os.remove(INDEX)
    run("gdaltindex", "-f", "GPKG", "-t_srs", "EPSG:4326", "-src_srs_name", "src_srs", "-lyr_name", "local", INDEX, *LOCAL)
    prefix = os.path.join("..", "stac", "naip-analytic", "")
    run(
        "ogr2ogr", "-update", "-nln", "s3", "-dialect", "SQLite", "-sql",
        f"SELECT '/vsis3/naip-analytic/' || substr(location, {len(prefix) + 1}) AS location, src_srs, geom"
        " FROM local ORDER BY fid",
        INDEX, INDEX,
    )
    run("ogr2ogr", "-update", "-nln", "utm", "-t_srs", "EPSG:26910", INDEX, INDEX, "local")


def warp_expected(tmp):
    z, x, y = TILE
    size = 2 * HALF / 2**z
    minx, maxy = -HALF + x * size, HALF - y * size
    rgbs = []
    for i, path in enumerate(reversed(LOCAL)):
        rgb = os.path.join(tmp, f"rgb{i}.vrt")
        run("gdal_translate", "-q", "-of", "VRT", "-b", "1", "-b", "2", "-b", "3", path, rgb)
        rgbs.append(rgb)
    warped = os.path.join(tmp, "warped.tif")
    run(
        "gdalwarp", "-q", "-overwrite", "-t_srs", "EPSG:3857",
        "-te", str(minx), str(maxy - size), str(minx + size), str(maxy), "-ts", "256", "256",
        "-r", "near", "-et", "0", "-ovr", "NONE", "-srcnodata", "0", "-dstalpha", "-wo", "UNIFIED_SRC_NODATA=YES",
        *rgbs, warped,
    )
    os.makedirs(os.path.join(HERE, "expected"), exist_ok=True)
    png = os.path.join(HERE, "expected", f"{z}_{x}_{y}.png")
    run("gdal_translate", "-q", "-of", "PNG", warped, png)
    if os.path.exists(png + ".aux.xml"):
        os.remove(png + ".aux.xml")


def main():
    write_index()
    with tempfile.TemporaryDirectory() as tmp:
        warp_expected(tmp)


if __name__ == "__main__":
    main()
