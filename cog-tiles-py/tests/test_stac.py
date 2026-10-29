import asyncio
import io
import json
import pickle
import re
from importlib.metadata import version
from pathlib import Path
from types import MappingProxyType

import numpy as np
import pytest
from PIL import Image

import cog_tiles

FIXTURES = Path(__file__).resolve().parents[2] / "tests" / "fixtures"
EXPECTED = FIXTURES / "cog" / "stac" / "expected"
SEARCH = FIXTURES / "render_cassette" / "earth-search.aws.element84.com" / "v1" / "search.post"


def decode(tile: bytes) -> np.ndarray:
    return np.asarray(Image.open(io.BytesIO(tile)))


def test_a_tile_is_painted_from_the_newest_items_a_search_finds(stac):
    naip = cog_tiles.open_stac(
        f"{stac.url}/v1", "naip", "image", 13, max_zoom=15, datetime="2020-01-01T00:00:00Z/..",
        query={"naip:state": {"eq": "ca"}}, nodata=0, options={"allow_http": "true"},
    )
    tile = decode(naip.tile(14, 2619, 6334))
    np.testing.assert_array_equal(tile, np.asarray(Image.open(EXPECTED / "14_2619_6334.png")))


def test_a_stac_source_reports_its_layout_and_the_collection_extent(stac):
    naip = cog_tiles.open_stac(
        f"{stac.url}/v1", "naip", "image", 13, max_zoom=15, datetime="2020-01-01T00:00:00Z/..",
        query={"naip:state": {"eq": "ca"}}, nodata=0, options={"allow_http": "true"},
    )
    assert isinstance(naip, cog_tiles.TileSource)
    assert (naip.min_zoom, naip.max_zoom, naip.tile_size, naip.format) == (13, 15, 256, "png")
    assert naip.bounds == (-160, 17, -67, 50)


def test_one_search_serves_every_tile_below_its_search_zoom_tile(stac):
    naip = cog_tiles.open_stac(
        f"{stac.url}/v1", "naip", "image", 13, max_zoom=15, datetime="2020-01-01T00:00:00Z/..",
        query={"naip:state": {"eq": "ca"}}, nodata=0, options={"allow_http": "true"},
    )
    assert naip.tile(13, 1309, 3167) is not None
    assert naip.tile(14, 2619, 6334) is not None
    assert naip.tile(14, 2618, 6335) is not None
    assert naip.tile(15, 5238, 12668) is not None
    assert len([search for search in stac.searches if "bbox" in search]) == 1


def test_a_search_paints_only_the_items_it_reads(stac):
    naip = cog_tiles.open_stac(
        f"{stac.url}/v1", "naip", "image", 13, max_zoom=15, max_items_per_search=3,
        datetime="2020-01-01T00:00:00Z/..", query={"naip:state": {"eq": "ca"}}, nodata=0,
        options={"allow_http": "true"},
    )
    tile = decode(naip.tile(14, 2619, 6334))
    np.testing.assert_array_equal(tile, np.asarray(Image.open(EXPECTED / "14_2619_6334_three_items.png")))


def test_a_tile_no_item_covers_is_none(stac):
    naip = cog_tiles.open_stac(
        f"{stac.url}/v1", "naip", "image", 13, datetime="2020-01-01T00:00:00Z/..",
        query={"naip:state": {"eq": "ca"}}, nodata=0, options={"allow_http": "true"},
    )
    assert naip.tile(13, 1283, 3177) is None


def test_bands_selected_by_name_match_the_band_numbers(stac):
    by_name = cog_tiles.open_stac(
        f"{stac.url}/v1", "naip", "image", 13, max_zoom=15, bands=["green"], datetime="2020-01-01T00:00:00Z/..",
        query={"naip:state": {"eq": "ca"}}, nodata=0, options={"allow_http": "true"},
    )
    expected = np.asarray(Image.open(EXPECTED / "14_2619_6334.png"))
    np.testing.assert_array_equal(by_name.tile_pixels(14, 2619, 6334), expected[..., [1, 3]])


def test_bands_selected_by_number_match_the_band_names(stac):
    by_number = cog_tiles.open_stac(
        f"{stac.url}/v1", "naip", "image", 13, max_zoom=15, bands=[2], datetime="2020-01-01T00:00:00Z/..",
        query={"naip:state": {"eq": "ca"}}, nodata=0, options={"allow_http": "true"},
    )
    expected = np.asarray(Image.open(EXPECTED / "14_2619_6334.png"))
    np.testing.assert_array_equal(by_number.tile_pixels(14, 2619, 6334), expected[..., [1, 3]])


def test_a_band_name_the_asset_lacks_is_rejected(stac):
    message = "Band purple is not a band of the image asset: it has red, green, blue, nir"
    with pytest.raises(ValueError, match=re.escape(message)):
        cog_tiles.open_stac(
            f"{stac.url}/v1", "naip", "image", 13, bands=["red", "green", "purple"],
            datetime="2020-01-01T00:00:00Z/..", query={"naip:state": {"eq": "ca"}}, nodata=0,
            options={"allow_http": "true"},
        )


def test_band_zero_is_rejected(stac):
    message = "Band 0 is not a band of the image asset: bands are counted from 1"
    with pytest.raises(ValueError, match=re.escape(message)):
        cog_tiles.open_stac(
            f"{stac.url}/v1", "naip", "image", 13, bands=[0], datetime="2020-01-01T00:00:00Z/..",
            query={"naip:state": {"eq": "ca"}}, nodata=0, options={"allow_http": "true"},
        )


def test_a_search_zoom_above_the_min_zoom_is_rejected(stac):
    message = "The min zoom 13 is below the search zoom 14"
    with pytest.raises(ValueError, match=re.escape(message)):
        cog_tiles.open_stac(
            f"{stac.url}/v1", "naip", "image", 13, search_zoom=14, datetime="2020-01-01T00:00:00Z/..",
            query={"naip:state": {"eq": "ca"}}, nodata=0, options={"allow_http": "true"},
        )


def test_an_unknown_order_is_rejected(stac):
    message = 'order must be "newest" or "oldest", not "random"'
    with pytest.raises(ValueError, match=re.escape(message)):
        cog_tiles.open_stac(
            f"{stac.url}/v1", "naip", "image", 13, order="random", datetime="2020-01-01T00:00:00Z/..",
            query={"naip:state": {"eq": "ca"}}, nodata=0, options={"allow_http": "true"},
        )


def test_lossless_jpeg_is_rejected(stac):
    with pytest.raises(ValueError, match=re.escape("JPEG has no lossless mode")):
        cog_tiles.open_stac(
            f"{stac.url}/v1", "naip", "image", 13, format="jpeg", quality="lossless",
            datetime="2020-01-01T00:00:00Z/..", query={"naip:state": {"eq": "ca"}}, nodata=0,
            options={"allow_http": "true"},
        )


def test_a_search_the_api_answers_with_404_is_an_error(stac):
    message = f'The STAC API at {stac.url}/v1/search answered 404: {{"code": "NotFound"}}'
    with pytest.raises(cog_tiles.CogTilesError, match=re.escape(message)):
        cog_tiles.open_stac(
            f"{stac.url}/v1", "naip", "image", 13, datetime="1999-01-01T00:00:00Z/..",
            query={"naip:state": {"eq": "ca"}}, nodata=0, options={"allow_http": "true"},
        )


def test_the_query_can_be_any_mapping(stac):
    naip = cog_tiles.open_stac(
        f"{stac.url}/v1", "naip", "image", 13, max_zoom=15, datetime="2020-01-01T00:00:00Z/..",
        query=MappingProxyType({"naip:state": {"eq": "ca"}}), nodata=0, options={"allow_http": "true"},
    )
    assert stac.searches[-1]["query"] == {"naip:state": {"eq": "ca"}}
    tile = decode(naip.tile(14, 2619, 6334))
    np.testing.assert_array_equal(tile, np.asarray(Image.open(EXPECTED / "14_2619_6334.png")))


def test_a_collection_without_items_is_rejected(stac):
    stac.answer = lambda body: (200, {"type": "FeatureCollection", "features": [], "links": []})
    message = f"No item of STAC collection naip matches the filters at {stac.url}/v1"
    with pytest.raises(cog_tiles.CogTilesError, match=re.escape(message)):
        cog_tiles.open_stac(
            f"{stac.url}/v1", "naip", "image", 13, datetime="2020-01-01T00:00:00Z/..",
            query={"naip:state": {"eq": "ca"}}, nodata=0, options={"allow_http": "true"},
        )


def test_a_next_link_outside_the_api_origin_fails_the_search(stac):
    naip = cog_tiles.open_stac(
        f"{stac.url}/v1", "naip", "image", 13, datetime="2020-01-01T00:00:00Z/..",
        query={"naip:state": {"eq": "ca"}}, nodata=0, options={"allow_http": "true"},
    )
    page = json.loads(stac.rewrite((SEARCH / "cd3b71590db07736.json").read_text()))
    page["numberMatched"] = page["context"]["matched"] = 9
    page["links"] = [{"rel": "next", "href": "http://elsewhere.invalid/search", "method": "GET"}]
    stac.answer = lambda body: (200, page)
    message = (
        f"Unable to search the mosaic index {stac.url}/v1/search: The STAC API at {stac.url}/v1 links its next"
        " search page to http://elsewhere.invalid/search, outside its origin"
    )
    with pytest.raises(cog_tiles.CogTilesError, match=re.escape(message)):
        naip.tile(13, 1309, 3167)


def test_searches_send_the_cog_tiles_py_user_agent(stac):
    cog_tiles.open_stac(
        f"{stac.url}/v1", "naip", "image", 13, datetime="2020-01-01T00:00:00Z/..",
        query={"naip:state": {"eq": "ca"}}, nodata=0, options={"allow_http": "true"},
    )
    assert stac.user_agents == {f"cog-tiles-py/{version('cog-tiles')}"}


def test_stac_sources_re_encode_tiles(stac):
    naip = cog_tiles.open_stac(
        f"{stac.url}/v1", "naip", "image", 13, max_zoom=15, format="webp", datetime="2020-01-01T00:00:00Z/..",
        query={"naip:state": {"eq": "ca"}}, nodata=0, options={"allow_http": "true"},
    )
    assert naip.format == "webp"
    assert naip.tile(14, 2619, 6334)[8:12] == b"WEBP"


def test_stac_sources_survive_pickling(stac):
    naip = cog_tiles.open_stac(
        f"{stac.url}/v1", "naip", "image", 13, max_zoom=15, bands=["blue", 2, "red"], max_items_per_search=3,
        search_expiry=60.0, format="jpeg", background="black", datetime="2020-01-01T00:00:00Z/..",
        query={"naip:state": {"eq": "ca"}}, nodata=0, options={"allow_http": "true"},
    )
    restored = pickle.loads(pickle.dumps(naip))
    assert repr(restored) == repr(naip)
    assert restored.tile(14, 2619, 6334) == naip.tile(14, 2619, 6334)


def test_open_stac_async_opens_the_same_source(stac):
    naip = asyncio.run(
        cog_tiles.open_stac_async(
            f"{stac.url}/v1", "naip", "image", 13, max_zoom=15, datetime="2020-01-01T00:00:00Z/..",
            query={"naip:state": {"eq": "ca"}}, nodata=0, options={"allow_http": "true"},
        )
    )
    tile = decode(asyncio.run(naip.tile_async(14, 2619, 6334)))
    np.testing.assert_array_equal(tile, np.asarray(Image.open(EXPECTED / "14_2619_6334.png")))
