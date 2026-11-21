"""Writes ocean.zip: a stand-in for osmdata.openstreetmap.de's water-polygons-split-3857.zip, laid out like it.

It holds one square in the Atlantic, 1/32 of the world wide, so 8 pixels at zoom 0, in EPSG:3857, with the fields `x`
and `y` of the original set to 1 and 2, as in the "ocean polygons" example of Planetiler's shortbread.spec.yml.
The .dbf and the zip entries have a fixed date, so the file is the same on every run.

    uv run --with pyshp python tests/fixtures/shapefile/ocean.py
"""

import io
import zipfile
from pathlib import Path

import shapefile

HERE = Path(__file__).parent
FOLDER = "water-polygons-split-3857"
PRJ = (
    'PROJCS["WGS_1984_Web_Mercator_Auxiliary_Sphere",GEOGCS["GCS_WGS_1984",DATUM["D_WGS_1984",'
    'SPHEROID["WGS_1984",6378137.0,298.257223563]],PRIMEM["Greenwich",0.0],UNIT["Degree",0.0174532925199433]],'
    'PROJECTION["Mercator_Auxiliary_Sphere"],PARAMETER["False_Easting",0.0],PARAMETER["False_Northing",0.0],'
    'PARAMETER["Central_Meridian",0.0],PARAMETER["Standard_Parallel_1",0.0],PARAMETER["Auxiliary_Sphere_Type",0.0],'
    'UNIT["Meter",1.0]]'
)
WEST, SOUTH, SIDE = -3_000_000.0, 0.0, 40_075_016.68557849 / 32

shp, shx, dbf = io.BytesIO(), io.BytesIO(), io.BytesIO()
with shapefile.Writer(shp=shp, shx=shx, dbf=dbf, shapeType=shapefile.POLYGON) as writer:
    writer.field("x", "N", 11, 0)
    writer.field("y", "N", 11, 0)
    east, north = WEST + SIDE, SOUTH + SIDE
    writer.poly([[(WEST, SOUTH), (WEST, north), (east, north), (east, SOUTH), (WEST, SOUTH)]])
    writer.record(1, 2)

table = bytearray(dbf.getvalue())
table[1:4] = bytes([126, 11, 15])
parts = {"shp": shp.getvalue(), "shx": shx.getvalue(), "dbf": bytes(table), "prj": PRJ.encode(), "cpg": b"UTF-8\n"}
with zipfile.ZipFile(HERE / "ocean.zip", "w") as archive:
    for ext, data in parts.items():
        entry = zipfile.ZipInfo(f"{FOLDER}/water_polygons.{ext}", (1980, 1, 1, 0, 0, 0))
        entry.compress_type = zipfile.ZIP_DEFLATED
        archive.writestr(entry, data)
