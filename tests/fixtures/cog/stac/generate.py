"""Write a small synthetic COG for every NAIP item the STAC cassette recorded.
Requires numpy and the GDAL command line tools.

    python3 tests/fixtures/cog/stac/generate.py

The real NAIP files are hundreds of megabytes in a requester-pays bucket, so each item's asset
`s3://naip-analytic/{key}` is stood in for by `naip-analytic/{key}` here: a 4-band COG in the
item's own UTM zone (`proj:epsg`) and extent (`proj:bbox`), with 30 m pixels. Red tells the
years apart (2022: 200, 2020: 90, other years: 140), green and blue are gradients across the
file, and the 2022 files leave their western 40% as 0 (nodata), so older items show through.

The e2e test records new searches on its first run off CI, then fails on the files they name;
run this script and the test again until both are quiet.
"""

import glob
import json
import math
import os
import subprocess
import tempfile

import numpy as np

HERE = os.path.dirname(os.path.abspath(__file__))
CASSETTE = os.path.join(HERE, "../../render_cassette/earth-search.aws.element84.com")
RES = 30.0
RED = {"2022": 200, "2020": 90}


def items():
    for path in glob.glob(os.path.join(CASSETTE, "**/*.json"), recursive=True):
        if path.endswith(".request.json"):
            continue
        with open(path) as f:
            page = json.load(f)
        yield from page.get("features", [])


def pixels(width, height, year):
    ys, xs = np.mgrid[0:height, 0:width]
    red = np.full((height, width), RED.get(year, 140), np.uint8)
    green = (xs * 4 % 200 + 30).astype(np.uint8)
    blue = (ys * 4 % 200 + 30).astype(np.uint8)
    nir = np.full((height, width), 255, np.uint8)
    data = np.stack([red, green, blue, nir])
    if year == "2022":
        data[:, :, : int(width * 0.4)] = 0
    return data


def write_cog(path, item, tmp):
    props = item["properties"]
    minx, miny, maxx, maxy = props["proj:bbox"]
    width, height = math.ceil((maxx - minx) / RES), math.ceil((maxy - miny) / RES)
    data = pixels(width, height, props["naip:year"])
    raw = os.path.join(tmp, "item.raw")
    np.ascontiguousarray(data.transpose(1, 2, 0)).tofile(raw)
    bands = "".join(
        f'<VRTRasterBand dataType="Byte" band="{b + 1}" subClass="VRTRawRasterBand">'
        f"<SourceFilename>{raw}</SourceFilename><ImageOffset>{b}</ImageOffset>"
        f"<PixelOffset>4</PixelOffset><LineOffset>{width * 4}</LineOffset></VRTRasterBand>"
        for b in range(4)
    )
    vrt = os.path.join(tmp, "item.vrt")
    with open(vrt, "w") as f:
        f.write(f'<VRTDataset rasterXSize="{width}" rasterYSize="{height}">{bands}</VRTDataset>')
    os.makedirs(os.path.dirname(path), exist_ok=True)
    subprocess.run(
        [
            "gdal_translate", "-q", "-of", "COG", "-a_srs", f"EPSG:{props['proj:epsg']}",
            "-a_ullr", str(minx), str(maxy), str(minx + width * RES), str(maxy - height * RES),
            "-co", "COMPRESS=DEFLATE", "-co", "BLOCKSIZE=256", vrt, path,
        ],
        check=True,
    )


def main():
    with tempfile.TemporaryDirectory() as tmp:
        for item in items():
            href = item["assets"]["image"]["href"]
            key = href.removeprefix("s3://naip-analytic/")
            path = os.path.join(HERE, "naip-analytic", key)
            if not os.path.exists(path):
                write_cog(path, item, tmp)
                print(path)


if __name__ == "__main__":
    main()
