"""Tests for a DuckDB/GeoParquet source, mirroring e2e-tests/tests/duckdb.rs.

Requires `martin` to be built with the `unstable-duckdb` feature (see
Cargo.toml); DuckDB is compiled from source on the first build, so this is
slower to build than the other source kinds.
"""

from __future__ import annotations

import json

import martin_py


def test_list_sources_uses_the_configured_layer_id(duckdb_server: martin_py.TileServer) -> None:
    assert duckdb_server.list_sources() == ["polygons"]


def test_get_source_info_describes_the_parquet_columns(duckdb_server: martin_py.TileServer) -> None:
    tilejson = json.loads(duckdb_server.get_source_info("polygons", "http://localhost/polygons"))

    assert tilejson["tilejson"] == "3.0.0"
    assert tilejson["name"] == "polygons"
    assert tilejson["minzoom"] == 0
    assert tilejson["maxzoom"] == 14
    assert tilejson["bounds"] == [-50.0, 20.0, 5.0, 30.0]
    assert tilejson["vector_layers"] == [
        {"id": "polygons", "fields": {"id": "INTEGER", "name": "VARCHAR"}}
    ]


def test_get_tile_clips_the_polygon_crossing_the_tile_edge(
    duckdb_server: martin_py.TileServer,
) -> None:
    data, content_type, _ = duckdb_server.get_tile("polygons", 1, 0, 0)

    assert isinstance(data, bytes)
    assert len(data) > 0
    assert content_type == "application/x-protobuf"
