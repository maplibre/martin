import asyncio
import io
import pickle
import re
import shutil
import sqlite3
from pathlib import Path

import numpy as np
import pytest
from PIL import Image

import cog_tiles

FIXTURES = Path(__file__).resolve().parents[2] / "tests" / "fixtures" / "cog"
NAIP = FIXTURES / "tile_index" / "naip.gpkg"
EXPECTED = FIXTURES / "tile_index" / "expected" / "14_2619_6334.png"


def decode(tile: bytes) -> np.ndarray:
    return np.asarray(Image.open(io.BytesIO(tile)))


def test_a_tile_is_painted_from_the_files_the_index_lists():
    index = cog_tiles.open_tile_index(NAIP, 13, layer="local", max_zoom=15, nodata=0)
    np.testing.assert_array_equal(decode(index.tile(14, 2619, 6334)), np.asarray(Image.open(EXPECTED)))


def test_a_tile_index_reports_its_layout_and_the_area_its_files_cover():
    index = cog_tiles.open_tile_index(NAIP, 13, layer="local", nodata=0)
    assert isinstance(index, cog_tiles.TileSource)
    assert (index.min_zoom, index.max_zoom, index.tile_size, index.format) == (13, 13, 256, "png")
    assert index.bounds == pytest.approx((-122.5027168972, 37.6852591659, -122.3719290312, 37.8146302227))
    min_x, min_y, max_x, max_y = index.mercator_bounds
    assert min_x == pytest.approx(-13_636_940.07, abs=0.01)
    assert min_x < max_x
    assert min_y < max_y


def test_a_tile_outside_the_zoom_range_is_none():
    index = cog_tiles.open_tile_index(NAIP, 14, layer="local", max_zoom=14, nodata=0)
    assert index.tile(13, 1309, 3167) is None
    assert index.tile(15, 5238, 12668) is None


def test_one_band_paints_gray_tiles_with_alpha():
    index = cog_tiles.open_tile_index(NAIP, 13, layer="local", max_zoom=15, bands=[2], nodata=0)
    pixels = index.tile_pixels(14, 2619, 6334)
    expected = np.asarray(Image.open(EXPECTED))
    np.testing.assert_array_equal(pixels, expected[..., [1, 3]])


def test_read_stitches_the_tiles_of_an_area():
    index = cog_tiles.open_tile_index(NAIP, 13, layer="local", max_zoom=15, nodata=0)
    raster = index.read(index.mercator_bounds, zoom=14, crs="EPSG:3857")
    assert raster.data.shape[2] == 4
    assert raster.data[..., 3].any()


def test_tiles_iterates_over_the_tiles_in_the_index_bounds():
    index = cog_tiles.open_tile_index(NAIP, 13, layer="local", nodata=0)
    assert sorted((z, x, y) for z, x, y, _ in index.tiles()) == [
        (13, x, y) for x in range(1308, 1312) for y in range(3165, 3169)
    ]


@pytest.mark.parametrize(
    ("arguments", "error", "message"),
    [
        ({"layer": "local", "bands": [0, 1, 2]}, ValueError, "bands are counted from 1"),
        ({"layer": "local", "bands": [1, 2]}, ValueError, "A mosaic serves 1 (gray) or 3 (RGB) bands, but 2 were selected"),
        ({}, ValueError, "has no single feature table, set `layer` to one of: local, s3, utm"),
        ({"layer": "nope"}, ValueError, "has no feature table nope"),
        ({"layer": "utm"}, cog_tiles.CogTilesError, "are in EPSG:26910, not EPSG:4326"),
        ({"layer": "local", "format": "gif"}, ValueError, 'format must be "png", "webp" or "jpeg", not "gif"'),
    ],
)
def test_invalid_tile_indexes_are_rejected(arguments, error, message):
    with pytest.raises(error, match=re.escape(message)):
        cog_tiles.open_tile_index(NAIP, 13, **arguments)


def test_a_missing_tile_index_is_file_not_found(tmp_path):
    with pytest.raises(FileNotFoundError, match="missing.gpkg"):
        cog_tiles.open_tile_index(tmp_path / "missing.gpkg", 13)


def test_tile_indexes_re_encode_tiles():
    index = cog_tiles.open_tile_index(NAIP, 13, layer="local", max_zoom=15, nodata=0, format="webp", quality="lossless")
    assert index.format == "webp"
    expected = np.asarray(Image.open(EXPECTED))
    assert (expected[..., 3] == 255).all()
    np.testing.assert_array_equal(decode(index.tile(14, 2619, 6334)), expected[..., :3])


def test_tile_indexes_survive_pickling():
    index = cog_tiles.open_tile_index(
        NAIP, 13, layer="local", max_zoom=15, bands=[3, 2, 1], nodata=0, max_cached_files=5, file_expiry=5.0, format="jpeg"
    )
    restored = pickle.loads(pickle.dumps(index))
    assert repr(restored) == repr(index)
    assert restored.tile(14, 2619, 6334) == index.tile(14, 2619, 6334)


def test_open_tile_index_async_opens_the_same_index():
    index = asyncio.run(cog_tiles.open_tile_index_async(NAIP, 13, layer="local", max_zoom=15, nodata=0))
    assert asyncio.run(index.tile_async(14, 2619, 6334)) == cog_tiles.open_tile_index(
        NAIP, 13, layer="local", max_zoom=15, nodata=0
    ).tile(14, 2619, 6334)


@pytest.mark.parametrize("prefix", ["", "/vsicurl/"])
def test_files_listed_by_url_are_read_over_http(tmp_path, fixture_server, prefix):
    index_path = tmp_path / "remote.gpkg"
    shutil.copy(NAIP, index_path)
    with sqlite3.connect(index_path) as db:
        for (trigger,) in db.execute("SELECT name FROM sqlite_master WHERE type = 'trigger' AND tbl_name = 'local'"):
            db.execute(f'DROP TRIGGER "{trigger}"')
        url = f"{prefix}http://127.0.0.1:{fixture_server.server_port}/"
        db.execute("UPDATE local SET location = ? || substr(location, 4)", (url,))
    index = cog_tiles.open_tile_index(
        index_path, 13, layer="local", max_zoom=15, nodata=0, options={"allow_http": "true"}
    )
    np.testing.assert_array_equal(decode(index.tile(14, 2619, 6334)), np.asarray(Image.open(EXPECTED)))
    assert fixture_server.gets > 0
