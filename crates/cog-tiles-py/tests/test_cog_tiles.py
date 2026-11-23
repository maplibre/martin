import multiprocessing
import pickle
import sys
import sysconfig
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

import numpy as np
import pytest
import tifffile

import cog_tiles

FIXTURES = Path(__file__).resolve().parents[2] / "tests" / "fixtures" / "cog"
SYNTHETIC = FIXTURES / "synthetic"
MOSAIC = FIXTURES / "mosaic"


def pattern(width: int, height: int, bands: int, left: int = 0, top: int = 0) -> np.ndarray:
    y, x, b = np.ogrid[top : top + height, left : left + width, :bands]
    return ((x + 2 * y + 64 * b) % 256).astype(np.uint8)


def test_open_reports_the_tile_layout():
    cog = cog_tiles.open(SYNTHETIC / "rgb_jpeg.tif")
    assert isinstance(cog, cog_tiles.TileSource)
    assert (cog.min_zoom, cog.max_zoom, cog.tile_size) == (15, 16, 256)
    assert (cog.format, cog.content_type) == ("jpg", "image/jpeg")
    west, south, east, north = cog.bounds
    assert west == pytest.approx(-0.10986328125)
    assert west < east
    assert south < north


def test_a_jpeg_cog_returns_jpeg_tiles():
    cog = cog_tiles.open(SYNTHETIC / "rgb_jpeg.tif")
    assert cog.tile(16, 32_748, 21_794).startswith(b"\xff\xd8")


def test_a_zstd_cog_returns_png_tiles():
    cog = cog_tiles.open(SYNTHETIC / "rgb_zstd.tif")
    assert cog.tile(16, 32_748, 21_794).startswith(b"\x89PNG\r\n\x1a\n")


def test_an_empty_tile_is_none():
    cog = cog_tiles.open(SYNTHETIC / "sparse.tif")
    assert cog.tile(16, 32_749, 21_795) is None


def test_a_tile_outside_the_bounds_is_none():
    cog = cog_tiles.open(SYNTHETIC / "sparse.tif")
    assert cog.tile(16, 32_753, 21_794) is None


def test_a_tile_below_the_min_zoom_is_none():
    cog = cog_tiles.open(SYNTHETIC / "sparse.tif")
    assert cog.tile(3, 0, 0) is None


def test_tile_pixels_are_the_decoded_tile():
    cog = cog_tiles.open(SYNTHETIC / "rgba_deflate.tif")
    pixels = cog.tile_pixels(16, 32_749, 21_794)
    assert pixels.dtype == np.uint8
    np.testing.assert_array_equal(pixels, pattern(256, 256, 4, left=256))


def test_webp_tiles_decode_too():
    cog = cog_tiles.open(SYNTHETIC / "rgb_webp.tif")
    pixels = cog.tile_pixels(16, 32_748, 21_794)
    assert np.abs(pixels.astype(np.int16) - pattern(256, 256, 3)).mean() < 4


def test_read_stitches_the_whole_image():
    cog = cog_tiles.open(SYNTHETIC / "rgb_zstd.tif")
    raster = cog.read(cog.bounds, zoom=16)
    np.testing.assert_array_equal(raster.data, pattern(512, 512, 3))
    assert raster.bounds == pytest.approx(cog.mercator_bounds)
    min_x, x_res, _, max_y, _, y_res = raster.transform
    assert (min_x, max_y) == pytest.approx((cog.mercator_bounds[0], cog.mercator_bounds[3]))
    assert x_res == pytest.approx(-y_res)
    assert (raster.zoom, raster.crs) == (16, "EPSG:3857")


def test_read_crops_to_the_pixels_the_bbox_touches():
    cog = cog_tiles.open(SYNTHETIC / "rgb_zstd.tif")
    min_x, _, _, max_y = cog.mercator_bounds
    res = 40_075_016.685_578_49 / 2**16 / 256
    bbox = (min_x + 100.5 * res, max_y - 300.5 * res, min_x + 400.5 * res, max_y - 50.5 * res)
    raster = cog.read(bbox, zoom=16, crs="EPSG:3857")
    np.testing.assert_array_equal(raster.data, pattern(301, 251, 3, left=100, top=50))


def test_read_defaults_to_the_highest_zoom():
    cog = cog_tiles.open(SYNTHETIC / "rgb_zstd.tif")
    assert cog.read(cog.bounds).zoom == 16


def test_read_without_data_is_none():
    cog = cog_tiles.open(SYNTHETIC / "rgb_zstd.tif")
    assert cog.read((10.0, 10.0, 10.01, 10.01), zoom=16) is None


def test_read_refuses_rasters_over_max_pixels():
    cog = cog_tiles.open(SYNTHETIC / "rgb_zstd.tif")
    with pytest.raises(ValueError, match="max_pixels"):
        cog.read(cog.bounds, zoom=16, max_pixels=1000)


def test_a_raster_converts_to_an_array():
    cog = cog_tiles.open(SYNTHETIC / "gray_lzw.tif")
    raster = cog.read(cog.bounds, zoom=15)
    assert np.asarray(raster).shape == (256, 256, 1)
    assert np.asarray(raster, dtype=np.float32).dtype == np.float32


def test_tiles_yields_every_tile_with_data():
    cog = cog_tiles.open(SYNTHETIC / "sparse.tif")
    tiles = {(z, x, y): data for z, x, y, data in cog.tiles(zoom=16)}
    assert tiles.keys() == {(16, 32_748, 21_794), (16, 32_749, 21_794), (16, 32_748, 21_795)}
    assert tiles[16, 32_748, 21_794] == cog.tile(16, 32_748, 21_794)


def test_tiles_defaults_to_every_zoom_of_the_source():
    cog = cog_tiles.open(SYNTHETIC / "rgb_zstd.tif")
    assert sorted((z, x, y) for z, x, y, _ in cog.tiles()) == [
        (15, 16_374, 10_897),
        (16, 32_748, 21_794),
        (16, 32_748, 21_795),
        (16, 32_749, 21_794),
        (16, 32_749, 21_795),
    ]


def test_tiles_outside_the_source_are_skipped():
    cog = cog_tiles.open(SYNTHETIC / "rgb_zstd.tif")
    assert not list(cog.tiles(bbox=(10.0, 10.0, 11.0, 11.0), zoom=range(31)))


def test_a_tile_iterator_can_be_dropped_early():
    cog = cog_tiles.open(SYNTHETIC / "rgb_zstd.tif")
    tiles = cog.tiles(concurrency=1)
    next(tiles)
    del tiles


def test_a_mosaic_reads_each_tile_from_its_file():
    mosaic = cog_tiles.open_mosaic(str(MOSAIC / "{quadkey}.tif"), file_zoom=16)
    assert (mosaic.min_zoom, mosaic.max_zoom, mosaic.file_zoom) == (16, 17, 16)
    assert mosaic.template == "{quadkey}.tif"
    assert mosaic.bounds is None
    single = cog_tiles.open(MOSAIC / "0313131311301121.tif")
    assert mosaic.tile(17, 65_499, 43_588) == single.tile(17, 65_499, 43_588)
    assert mosaic.tile(16, 32_750, 21_794) is None


def test_a_mosaic_read_stitches_across_files():
    mosaic = cog_tiles.open_mosaic(str(MOSAIC / "{quadkey}.tif"), file_zoom=16)
    nw = cog_tiles.open(MOSAIC / "0313131311301120.tif")
    se = cog_tiles.open(MOSAIC / "0313131311301123.tif")
    west, _, _, north = nw.bounds
    _, south, east, _ = se.bounds
    raster = mosaic.read((west, south, east, north), zoom=16)
    assert raster.data.shape[:2] == (512, 512)
    np.testing.assert_array_equal(raster.data[256:, 256:], se.read(se.bounds, zoom=16).data)


def test_a_mosaic_places_untagged_striped_files_by_their_name(tmp_path):
    for quadkey in ["0230102033312300", "0230102033312301"]:
        with tifffile.TiffWriter(tmp_path / f"{quadkey}_rgb.tif") as tif:
            tif.write(pattern(512, 512, 3), photometric="rgb", rowsperstrip=80, subfiletype=2, metadata=None)
            tif.write(pattern(256, 256, 3, left=1), photometric="rgb", rowsperstrip=256, subfiletype=2, metadata=None)
    mosaic = cog_tiles.open_mosaic(str(tmp_path / "{quadkey}_rgb.tif"), file_zoom=16)

    raster = mosaic.read(
        (-13626581.906454941, 4551366.412212535, -13625358.914002378, 4551977.908438817),
        zoom=17,
        crs="EPSG:3857",
    )

    assert (mosaic.min_zoom, mosaic.max_zoom) == (16, 17)
    np.testing.assert_array_equal(raster.data, np.concatenate([pattern(512, 512, 3), pattern(512, 512, 3)], axis=1))


def test_a_mosaic_needs_a_bbox_to_iterate():
    mosaic = cog_tiles.open_mosaic(str(MOSAIC / "{quadkey}.tif"), file_zoom=16)
    with pytest.raises(ValueError, match="bbox"):
        mosaic.tiles()


def test_cogs_survive_pickling():
    cog = cog_tiles.open(SYNTHETIC / "rgb_jpeg.tif")
    restored = pickle.loads(pickle.dumps(cog))
    assert repr(restored) == repr(cog)
    assert restored.tile(16, 32_748, 21_794) == cog.tile(16, 32_748, 21_794)


def test_mosaics_survive_pickling():
    mosaic = cog_tiles.open_mosaic(str(MOSAIC / "{quadkey}.tif"), 16, max_cached_files=10, file_expiry=5.0)
    restored = pickle.loads(pickle.dumps(mosaic))
    assert repr(restored) == repr(mosaic)
    assert restored.tile(17, 65_499, 43_588) == mosaic.tile(17, 65_499, 43_588)


def test_threads_read_concurrently():
    cog = cog_tiles.open(SYNTHETIC / "rgb_zstd.tif")
    with ThreadPoolExecutor(max_workers=8) as pool:
        first, *rest = pool.map(lambda _: cog.read(cog.bounds, zoom=16).data, range(32))
    for data in rest:
        np.testing.assert_array_equal(data, first)


@pytest.mark.skipif(not sysconfig.get_config_var("Py_GIL_DISABLED"), reason="needs free-threaded Python")
def test_importing_on_free_threaded_python_keeps_the_gil_disabled():
    assert not sys._is_gil_enabled()


def _read_in_child(cog, queue):
    queue.put(cog.tile(16, 32_748, 21_794))


@pytest.mark.filterwarnings("ignore:This process .* is multi-threaded:DeprecationWarning")
def test_a_forked_child_reads_with_its_own_runtime():
    cog = cog_tiles.open(SYNTHETIC / "rgb_zstd.tif")
    expected = cog.tile(16, 32_748, 21_794)
    context = multiprocessing.get_context("fork")
    queue = context.Queue()
    child = context.Process(target=_read_in_child, args=(cog, queue))
    child.start()
    assert queue.get(timeout=30) == expected
    child.join(timeout=30)
    assert child.exitcode == 0


def test_a_missing_file_raises_file_not_found():
    with pytest.raises(FileNotFoundError):
        cog_tiles.open(SYNTHETIC / "missing.tif")


def test_an_unsupported_file_says_why():
    with pytest.raises(cog_tiles.UnsupportedCogError, match="EPSG:32630"):
        cog_tiles.open(SYNTHETIC / "utm.tif")


def test_a_tile_past_the_zoom_raises_value_error():
    cog = cog_tiles.open(SYNTHETIC / "rgb_zstd.tif")
    with pytest.raises(ValueError):
        cog.tile(16, 65_536, 0)


def test_an_inverted_bbox_raises_value_error():
    cog = cog_tiles.open(SYNTHETIC / "rgb_zstd.tif")
    with pytest.raises(ValueError):
        cog.read((1.0, 0.0, 0.0, 1.0))


def test_an_unknown_crs_raises_value_error():
    cog = cog_tiles.open(SYNTHETIC / "rgb_zstd.tif")
    with pytest.raises(ValueError):
        cog.read(cog.bounds, crs="EPSG:4269")


def test_zero_concurrency_raises_value_error():
    cog = cog_tiles.open(SYNTHETIC / "rgb_zstd.tif")
    with pytest.raises(ValueError):
        cog.tiles(concurrency=0)


def test_an_invalid_template_raises_value_error():
    with pytest.raises(ValueError, match="placeholder"):
        cog_tiles.open_mosaic(str(MOSAIC / "{q}.tif"), 16)


def test_a_cog_over_http_returns_the_tiles_of_the_local_file(server):
    cog = cog_tiles.open(f"http://127.0.0.1:{server.server_port}/rgb_zstd.tif", options={"allow_http": "true"})
    local = cog_tiles.open(SYNTHETIC / "rgb_zstd.tif")
    assert cog.tile(16, 32_748, 21_794) == local.tile(16, 32_748, 21_794)
