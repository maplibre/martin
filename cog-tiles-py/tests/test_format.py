import asyncio
import io
import pickle
import re
from pathlib import Path

import numpy as np
import pytest
from PIL import Image

import cog_tiles

FIXTURES = Path(__file__).resolve().parents[2] / "tests" / "fixtures" / "cog"
SYNTHETIC = FIXTURES / "synthetic"
MOSAIC = FIXTURES / "mosaic"


def test_format_webp_re_encodes_png_tiles_as_webp():
    cog = cog_tiles.open(SYNTHETIC / "rgb_zstd.tif", format="webp")
    assert (cog.format, cog.content_type) == ("webp", "image/webp")
    tile = cog.tile(16, 32_748, 21_794)
    assert tile[:4] == b"RIFF"
    assert tile[8:12] == b"WEBP"


def decode(tile: bytes) -> np.ndarray:
    return np.asarray(Image.open(io.BytesIO(tile)))


def test_lossless_webp_decodes_to_the_tile_pixels():
    cog = cog_tiles.open(SYNTHETIC / "rgb_zstd.tif", format="webp", quality="lossless")
    np.testing.assert_array_equal(decode(cog.tile(16, 32_748, 21_794)), cog.tile_pixels(16, 32_748, 21_794))


def over(pixels: np.ndarray, background: tuple[int, int, int]) -> np.ndarray:
    alpha = pixels[..., 3:].astype(np.float64) / 255
    return pixels[..., :3] * alpha + np.array(background) * (1 - alpha)


def test_jpeg_paints_transparent_pixels_over_white():
    cog = cog_tiles.open(SYNTHETIC / "rgba_deflate.tif", format="jpeg")
    assert (cog.format, cog.content_type) == ("jpg", "image/jpeg")
    tile = cog.tile(16, 32_749, 21_794)
    assert tile.startswith(b"\xff\xd8")
    expected = over(cog.tile_pixels(16, 32_749, 21_794), (255, 255, 255))
    assert np.abs(decode(tile) - expected).mean() < 4


@pytest.mark.parametrize(("background", "rgb"), [("black", (0, 0, 0)), ("#ff8000", (255, 128, 0)), ((0, 64, 255), (0, 64, 255))])
def test_jpeg_paints_transparent_pixels_over_the_background(background, rgb):
    cog = cog_tiles.open(SYNTHETIC / "rgba_deflate.tif", format="jpeg", background=background)
    expected = over(cog.tile_pixels(16, 32_749, 21_794), rgb)
    assert np.abs(decode(cog.tile(16, 32_749, 21_794)) - expected).mean() < 4


@pytest.mark.parametrize(
    ("arguments", "message"),
    [
        ({"format": "gif"}, 'format must be "png", "webp" or "jpeg", not "gif"'),
        ({"quality": 80}, "quality needs format webp or jpeg"),
        ({"format": "png", "quality": 80}, "quality needs format webp or jpeg"),
        ({"format": "webp", "quality": 0}, "quality must be 1 to 100, not 0"),
        ({"format": "webp", "quality": 101}, "quality must be 1 to 100, not 101"),
        ({"format": "webp", "quality": "best"}, 'quality must be 1 to 100 or "lossless", not "best"'),
        ({"format": "jpeg", "quality": "lossless"}, "JPEG has no lossless mode, so quality must be 1 to 100"),
        ({"background": "black"}, "background needs format jpeg"),
        ({"format": "webp", "background": "black"}, "background needs format jpeg"),
        ({"format": "jpeg", "background": "transparent"}, 'background must be an opaque CSS color, not "transparent"'),
        ({"format": "jpeg", "background": "nope"}, 'background "nope": '),
    ],
)
def test_invalid_encodings_are_rejected(arguments, message):
    with pytest.raises(ValueError, match=re.escape(message)):
        cog_tiles.open(SYNTHETIC / "rgb_zstd.tif", **arguments)


@pytest.mark.parametrize("fmt", ["webp", "jpeg"])
def test_a_lower_quality_makes_smaller_tiles(fmt):
    low = cog_tiles.open(SYNTHETIC / "rgb_zstd.tif", format=fmt, quality=10)
    high = cog_tiles.open(SYNTHETIC / "rgb_zstd.tif", format=fmt, quality=95)
    assert len(low.tile(16, 32_748, 21_794)) < len(high.tile(16, 32_748, 21_794)) / 2


def test_format_png_re_encodes_jpeg_tiles_losslessly():
    cog = cog_tiles.open(SYNTHETIC / "rgb_jpeg.tif", format="png")
    assert cog.format == "png"
    tile = cog.tile(16, 32_748, 21_794)
    assert tile.startswith(b"\x89PNG\r\n\x1a\n")
    np.testing.assert_array_equal(decode(tile), cog.tile_pixels(16, 32_748, 21_794))


def test_an_empty_tile_stays_none_when_re_encoded():
    cog = cog_tiles.open(SYNTHETIC / "sparse.tif", format="webp")
    assert cog.tile(16, 32_749, 21_795) is None


def test_tiles_iterates_over_re_encoded_tiles():
    cog = cog_tiles.open(SYNTHETIC / "rgb_zstd.tif", format="webp")
    tiles = list(cog.tiles(zoom=16))
    assert len(tiles) == 4
    assert all(data[8:12] == b"WEBP" for _, _, _, data in tiles)


def test_open_async_re_encodes_tiles():
    cog = asyncio.run(cog_tiles.open_async(SYNTHETIC / "rgb_zstd.tif", format="jpeg", quality=50, background="black"))
    assert cog.format == "jpg"
    assert asyncio.run(cog.tile_async(16, 32_748, 21_794)).startswith(b"\xff\xd8")


def test_re_encoded_cogs_survive_pickling():
    cog = cog_tiles.open(SYNTHETIC / "rgba_deflate.tif", format="jpeg", quality=50, background=(0, 64, 255))
    restored = pickle.loads(pickle.dumps(cog))
    assert restored.format == "jpg"
    assert restored.tile(16, 32_749, 21_794) == cog.tile(16, 32_749, 21_794)


def test_mosaics_re_encode_tiles():
    mosaic = cog_tiles.open_mosaic(str(MOSAIC / "{quadkey}.tif"), 16, format="webp", quality="lossless")
    assert mosaic.format == "webp"
    tile = mosaic.tile(17, 65_499, 43_588)
    assert tile[8:12] == b"WEBP"
    np.testing.assert_array_equal(decode(tile), mosaic.tile_pixels(17, 65_499, 43_588))


def test_re_encoded_mosaics_survive_pickling():
    mosaic = cog_tiles.open_mosaic(str(MOSAIC / "{quadkey}.tif"), 16, format="jpeg", background="black")
    restored = pickle.loads(pickle.dumps(mosaic))
    assert restored.format == "jpg"
    assert restored.tile(17, 65_499, 43_588) == mosaic.tile(17, 65_499, 43_588)


def test_open_mosaic_async_re_encodes_tiles():
    mosaic = asyncio.run(cog_tiles.open_mosaic_async(str(MOSAIC / "{quadkey}.tif"), 16, format="png"))
    assert mosaic.format == "png"
    assert mosaic.tile(17, 65_499, 43_588).startswith(b"\x89PNG\r\n\x1a\n")


def test_mosaics_reject_invalid_encodings():
    with pytest.raises(ValueError, match="background needs format jpeg"):
        cog_tiles.open_mosaic(str(MOSAIC / "{quadkey}.tif"), 16, format="webp", background="black")
