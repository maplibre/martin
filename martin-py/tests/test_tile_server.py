"""Tests for `martin_py.TileServer`, covering the same ground as Martin's
`/{source_ids}` and `/{source_ids}/{z}/{x}/{y}` HTTP endpoints, but in-process.
"""

from __future__ import annotations

import json
from pathlib import Path

import pytest

import martin_py


def test_constructor_rejects_missing_config_file() -> None:
    with pytest.raises(RuntimeError):
        martin_py.TileServer("/no/such/config.yaml")


def test_constructor_rejects_config_with_no_sources(tmp_path: Path) -> None:
    empty_config = tmp_path / "empty.yaml"
    empty_config.write_text("{}\n")
    with pytest.raises(RuntimeError):
        martin_py.TileServer(str(empty_config))


class TestGeoJsonSource:
    def test_list_sources(self, geojson_server: martin_py.TileServer) -> None:
        assert geojson_server.list_sources() == ["geojson_a"]

    def test_get_source_info_returns_tilejson(self, geojson_server: martin_py.TileServer) -> None:
        tilejson = json.loads(geojson_server.get_source_info("geojson_a", "http://localhost/geojson_a"))

        assert tilejson["tilejson"] == "3.0.0"
        assert tilejson["tiles"] == ["http://localhost/geojson_a/{z}/{x}/{y}"]
        assert "vector_layers" in tilejson

    def test_get_source_info_unknown_source_raises_value_error(
        self, geojson_server: martin_py.TileServer
    ) -> None:
        with pytest.raises(ValueError):
            geojson_server.get_source_info("does_not_exist")

    def test_get_tile_returns_mvt_bytes(self, geojson_server: martin_py.TileServer) -> None:
        data, content_type, content_encoding = geojson_server.get_tile("geojson_a", 0, 0, 0)

        assert isinstance(data, bytes)
        assert len(data) > 0
        assert content_type == "application/x-protobuf"
        # A freshly-computed in-process tile isn't compressed.
        assert content_encoding is None

    def test_get_tile_is_deterministic(self, geojson_server: martin_py.TileServer) -> None:
        first, _, _ = geojson_server.get_tile("geojson_a", 0, 0, 0)
        second, _, _ = geojson_server.get_tile("geojson_a", 0, 0, 0)
        assert first == second

    def test_get_tile_unknown_source_raises_value_error(
        self, geojson_server: martin_py.TileServer
    ) -> None:
        with pytest.raises(ValueError):
            geojson_server.get_tile("does_not_exist", 0, 0, 0)


class TestCompositeSources:
    """`source_ids` may comma-join several sources into one composite request,
    mirroring the `/{source_ids}` and `/{source_ids}/{z}/{x}/{y}` HTTP routes.
    """

    def test_list_sources_includes_both(self, multi_geojson_server: martin_py.TileServer) -> None:
        assert sorted(multi_geojson_server.list_sources()) == ["geojson_a", "geojson_b"]

    def test_get_source_info_merges_both_sources(
        self, multi_geojson_server: martin_py.TileServer
    ) -> None:
        tilejson = json.loads(multi_geojson_server.get_source_info("geojson_a,geojson_b"))
        layer_ids = {layer["id"] for layer in tilejson["vector_layers"]}
        assert layer_ids == {"geojson_a", "geojson_b"}

    def test_get_tile_combines_both_sources(self, multi_geojson_server: martin_py.TileServer) -> None:
        data, content_type, _ = multi_geojson_server.get_tile("geojson_a,geojson_b", 0, 0, 0)
        assert len(data) > 0
        assert content_type == "application/x-protobuf"


class TestPmtilesSource:
    def test_get_tile_returns_png_bytes(self, pmtiles_server: martin_py.TileServer) -> None:
        data, content_type, _ = pmtiles_server.get_tile("pmtiles_a", 0, 0, 0)

        assert isinstance(data, bytes)
        assert len(data) > 0
        assert content_type == "image/png"
        # PNG magic bytes.
        assert data[:8] == b"\x89PNG\r\n\x1a\n"


class TestZoomBounds:
    def test_tile_within_supported_zoom_range_succeeds(
        self, bounded_zoom_server: martin_py.TileServer
    ) -> None:
        tilejson = json.loads(bounded_zoom_server.get_source_info("stamen_z3"))
        maxzoom = tilejson["maxzoom"]

        data, _, _ = bounded_zoom_server.get_tile("stamen_z3", maxzoom, 0, 0)
        assert len(data) > 0

    def test_tile_beyond_supported_zoom_range_raises_value_error(
        self, bounded_zoom_server: martin_py.TileServer
    ) -> None:
        tilejson = json.loads(bounded_zoom_server.get_source_info("stamen_z3"))
        maxzoom = tilejson["maxzoom"]

        with pytest.raises(ValueError):
            bounded_zoom_server.get_tile("stamen_z3", maxzoom + 1, 0, 0)
