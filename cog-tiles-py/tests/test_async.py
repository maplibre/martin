import asyncio
import time
from pathlib import Path

import numpy as np
import pytest

import cog_tiles

FIXTURES = Path(__file__).resolve().parents[2] / "tests" / "fixtures" / "cog"
SYNTHETIC = FIXTURES / "synthetic"
MOSAIC = FIXTURES / "mosaic"


def test_open_async_opens_the_cog_open_opens():
    cog = asyncio.run(cog_tiles.open_async(SYNTHETIC / "rgb_zstd.tif"))
    expected = cog_tiles.open(SYNTHETIC / "rgb_zstd.tif")
    assert repr(cog) == repr(expected)


def test_open_mosaic_async_opens_the_mosaic_open_mosaic_opens():
    mosaic = asyncio.run(cog_tiles.open_mosaic_async(str(MOSAIC / "{quadkey}.tif"), file_zoom=16))
    expected = cog_tiles.open_mosaic(str(MOSAIC / "{quadkey}.tif"), file_zoom=16)
    assert repr(mosaic) == repr(expected)


def test_tile_async_returns_the_tile_tile_returns():
    cog = cog_tiles.open(SYNTHETIC / "rgb_zstd.tif")
    assert asyncio.run(cog.tile_async(16, 32_748, 21_794)) == cog.tile(16, 32_748, 21_794)


def test_tile_async_returns_none_for_an_empty_tile():
    cog = cog_tiles.open(SYNTHETIC / "sparse.tif")
    assert asyncio.run(cog.tile_async(16, 32_749, 21_795)) is None


def test_tile_pixels_async_returns_the_pixels_tile_pixels_returns():
    cog = cog_tiles.open(SYNTHETIC / "rgba_deflate.tif")
    pixels = asyncio.run(cog.tile_pixels_async(16, 32_749, 21_794))
    np.testing.assert_array_equal(pixels, cog.tile_pixels(16, 32_749, 21_794))


def test_read_async_returns_the_raster_read_returns():
    cog = cog_tiles.open(SYNTHETIC / "rgb_zstd.tif")
    raster = asyncio.run(cog.read_async(cog.bounds, zoom=16))
    expected = cog.read(cog.bounds, zoom=16)
    np.testing.assert_array_equal(raster.data, expected.data)
    assert (raster.transform, raster.zoom) == (expected.transform, expected.zoom)


def test_read_async_raises_value_error_for_an_unknown_crs():
    cog = cog_tiles.open(SYNTHETIC / "rgb_zstd.tif")
    with pytest.raises(ValueError, match="EPSG:4269"):
        asyncio.run(cog.read_async(cog.bounds, crs="EPSG:4269"))


def test_async_for_yields_the_tiles_for_yields():
    cog = cog_tiles.open(SYNTHETIC / "sparse.tif")

    async def collect():
        return sorted([tile async for tile in cog.tiles(zoom=16)])

    assert asyncio.run(collect()) == sorted(cog.tiles(zoom=16))


def test_tile_async_calls_run_concurrently_on_one_event_loop():
    cog = cog_tiles.open(SYNTHETIC / "rgb_zstd.tif")

    async def gather():
        return await asyncio.gather(*(cog.tile_async(16, 32_748, 21_794) for _ in range(32)))

    assert asyncio.run(gather()) == [cog.tile(16, 32_748, 21_794)] * 32


def test_a_cancelled_read_async_raises_cancelled_error():
    cog = cog_tiles.open(SYNTHETIC / "rgb_zstd.tif")

    async def cancel():
        task = asyncio.ensure_future(cog.read_async(cog.bounds, zoom=16))
        await asyncio.sleep(0)
        task.cancel()
        with pytest.raises(asyncio.CancelledError):
            await task

    asyncio.run(cancel())


def test_tile_async_leaves_the_event_loop_free_while_it_fetches(server):
    cog = cog_tiles.open(f"http://127.0.0.1:{server.server_port}/rgb_zstd.tif", options={"allow_http": "true"})
    server.holding = True

    async def fetch():
        tile = asyncio.ensure_future(cog.tile_async(16, 32_748, 21_794))
        await asyncio.to_thread(server.held.wait, 5)
        server.released.set()
        return await tile

    tile = asyncio.run(fetch())
    expected = cog_tiles.open(SYNTHETIC / "rgb_zstd.tif").tile(16, 32_748, 21_794)
    assert tile == expected
    assert server.served_on_release


def test_cancelling_read_async_stops_its_fetches(server):
    cog = cog_tiles.open(f"http://127.0.0.1:{server.server_port}/rgb_zstd.tif", options={"allow_http": "true"})
    server.gets = 0
    server.holding = True

    async def cancel():
        read = asyncio.ensure_future(cog.read_async(cog.bounds, zoom=16, concurrency=1))
        await asyncio.to_thread(server.held.wait, 5)
        read.cancel()
        await asyncio.gather(read, return_exceptions=True)

    asyncio.run(cancel())
    server.released.set()
    time.sleep(0.5)
    assert server.gets == 1
