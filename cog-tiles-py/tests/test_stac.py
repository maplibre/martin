import asyncio
import io
import json
import pickle
import re
from pathlib import Path
from types import MappingProxyType

import numpy as np
import pytest
from PIL import Image

import cog_tiles

FIXTURES = Path(__file__).resolve().parents[2] / "tests" / "fixtures"
EXPECTED = FIXTURES / "cog" / "stac" / "expected"
SEARCH = FIXTURES / "render_cassette" / "earth-search.aws.element84.com" / "v1" / "search.post"
NAIP = {"datetime": "2020-01-01T00:00:00Z/..", "query": {"naip:state": {"eq": "ca"}}, "nodata": 0}
HTTP = {"allow_http": "true"}


def decode(tile: bytes) -> np.ndarray:
    return np.asarray(Image.open(io.BytesIO(tile)))


def test_a_tile_is_painted_from_the_newest_items_a_search_finds(stac):
    naip = cog_tiles.open_stac(f"{stac.url}/v1", "naip", "image", 13, max_zoom=15, options=HTTP, **NAIP)
    tile = decode(naip.tile(14, 2619, 6334))
    np.testing.assert_array_equal(tile, np.asarray(Image.open(EXPECTED / "14_2619_6334.png")))


def test_a_stac_source_reports_its_layout_and_the_collection_extent(stac):
    naip = cog_tiles.open_stac(f"{stac.url}/v1", "naip", "image", 13, max_zoom=15, options=HTTP, **NAIP)
    assert isinstance(naip, cog_tiles.TileSource)
    assert (naip.min_zoom, naip.max_zoom, naip.tile_size, naip.format) == (13, 15, 256, "png")
    assert naip.bounds == (-160, 17, -67, 50)


def test_one_search_serves_every_tile_below_its_search_zoom_tile(stac):
    naip = cog_tiles.open_stac(f"{stac.url}/v1", "naip", "image", 13, max_zoom=15, options=HTTP, **NAIP)
    for z, x, y in [(13, 1309, 3167), (14, 2619, 6334), (14, 2618, 6335), (15, 5238, 12668)]:
        assert naip.tile(z, x, y) is not None
    bbox_searches = [search for search in stac.searches if "bbox" in search]
    assert len(bbox_searches) == 1


def test_a_search_paints_only_the_items_it_reads(stac):
    naip = cog_tiles.open_stac(
        f"{stac.url}/v1", "naip", "image", 13, max_zoom=15, max_items_per_search=3, options=HTTP, **NAIP
    )
    tile = decode(naip.tile(14, 2619, 6334))
    np.testing.assert_array_equal(tile, np.asarray(Image.open(EXPECTED / "14_2619_6334_three_items.png")))


def test_a_tile_no_item_covers_is_none(stac):
    naip = cog_tiles.open_stac(f"{stac.url}/v1", "naip", "image", 13, options=HTTP, **NAIP)
    assert naip.tile(13, 1283, 3177) is None


def test_bands_are_selected_by_name_or_number(stac):
    by_name = cog_tiles.open_stac(f"{stac.url}/v1", "naip", "image", 13, max_zoom=15, bands=["green"], options=HTTP, **NAIP)
    by_number = cog_tiles.open_stac(f"{stac.url}/v1", "naip", "image", 13, max_zoom=15, bands=[2], options=HTTP, **NAIP)
    expected = np.asarray(Image.open(EXPECTED / "14_2619_6334.png"))
    np.testing.assert_array_equal(by_name.tile_pixels(14, 2619, 6334), expected[..., [1, 3]])
    np.testing.assert_array_equal(by_number.tile_pixels(14, 2619, 6334), expected[..., [1, 3]])


@pytest.mark.parametrize(
    ("arguments", "error", "message"),
    [
        ({"bands": ["red", "green", "purple"]}, ValueError, "Band purple is not a band of the image asset: it has red, green, blue, nir"),
        ({"bands": [0]}, ValueError, "Band 0 is not a band of the image asset: bands are counted from 1"),
        ({"search_zoom": 14}, ValueError, "The min zoom 13 is below the search zoom 14"),
        ({"order": "random"}, ValueError, 'order must be "newest" or "oldest", not "random"'),
        ({"format": "jpeg", "quality": "lossless"}, ValueError, "JPEG has no lossless mode"),
        ({"datetime": "1999-01-01T00:00:00Z/.."}, cog_tiles.CogTilesError, "answered 404"),
    ],
)
def test_invalid_stac_sources_are_rejected(stac, arguments, error, message):
    with pytest.raises(error, match=re.escape(message)):
        cog_tiles.open_stac(f"{stac.url}/v1", "naip", "image", 13, options=HTTP, **{**NAIP, **arguments})


def test_the_query_can_be_any_mapping(stac):
    arguments = {**NAIP, "query": MappingProxyType(NAIP["query"])}
    naip = cog_tiles.open_stac(f"{stac.url}/v1", "naip", "image", 13, max_zoom=15, options=HTTP, **arguments)
    assert stac.searches[-1]["query"] == NAIP["query"]
    tile = decode(naip.tile(14, 2619, 6334))
    np.testing.assert_array_equal(tile, np.asarray(Image.open(EXPECTED / "14_2619_6334.png")))


def test_a_collection_without_items_is_rejected(stac):
    stac.answer = lambda body: (200, {"type": "FeatureCollection", "features": [], "links": []})
    with pytest.raises(cog_tiles.CogTilesError, match="No item of STAC collection naip matches the filters"):
        cog_tiles.open_stac(f"{stac.url}/v1", "naip", "image", 13, options=HTTP, **NAIP)


def test_a_next_link_outside_the_api_origin_fails_the_search(stac):
    naip = cog_tiles.open_stac(f"{stac.url}/v1", "naip", "image", 13, options=HTTP, **NAIP)
    page = json.loads(stac.rewrite((SEARCH / "cd3b71590db07736.json").read_text()))
    page["numberMatched"] = page["context"]["matched"] = 9
    page["links"] = [{"rel": "next", "href": "http://elsewhere.invalid/search", "method": "GET"}]
    stac.answer = lambda body: (200, page)
    with pytest.raises(cog_tiles.CogTilesError, match="outside its origin"):
        naip.tile(13, 1309, 3167)


def test_the_api_is_told_who_asks(stac):
    cog_tiles.open_stac(f"{stac.url}/v1", "naip", "image", 13, options=HTTP, **NAIP)
    assert len(stac.user_agents) == 1
    assert next(iter(stac.user_agents)).startswith("cog-tiles-py/")


def test_stac_sources_re_encode_tiles(stac):
    naip = cog_tiles.open_stac(f"{stac.url}/v1", "naip", "image", 13, max_zoom=15, format="webp", options=HTTP, **NAIP)
    assert naip.format == "webp"
    assert naip.tile(14, 2619, 6334)[8:12] == b"WEBP"


def test_stac_sources_survive_pickling(stac):
    naip = cog_tiles.open_stac(
        f"{stac.url}/v1", "naip", "image", 13, max_zoom=15, bands=["blue", 2, "red"], max_items_per_search=3,
        search_expiry=60.0, format="jpeg", background="black", options=HTTP, **NAIP,
    )
    restored = pickle.loads(pickle.dumps(naip))
    assert repr(restored) == repr(naip)
    assert restored.tile(14, 2619, 6334) == naip.tile(14, 2619, 6334)


def test_open_stac_async_opens_the_same_source(stac):
    naip = asyncio.run(cog_tiles.open_stac_async(f"{stac.url}/v1", "naip", "image", 13, max_zoom=15, options=HTTP, **NAIP))
    tile = decode(asyncio.run(naip.tile_async(14, 2619, 6334)))
    np.testing.assert_array_equal(tile, np.asarray(Image.open(EXPECTED / "14_2619_6334.png")))
