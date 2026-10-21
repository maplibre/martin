"""Shared fixtures for the `martin_py` test suite.

These tests exercise the compiled `martin_py` extension module (see
`../justfile` for how to build it into a venv with `maturin develop`), driving
it against the same fixture files the Rust end-to-end tests use.
"""

from __future__ import annotations

from pathlib import Path

import pytest
import yaml

import martin_py

REPO_ROOT = Path(__file__).resolve().parents[2]
FIXTURES = REPO_ROOT / "tests" / "fixtures"


def write_config(tmp_path: Path, config: dict) -> Path:
    """Write `config` as YAML and return its path, for `martin_py.TileServer`."""
    config_path = tmp_path / "config.yaml"
    config_path.write_text(yaml.safe_dump(config))
    return config_path


@pytest.fixture
def geojson_server(tmp_path: Path) -> martin_py.TileServer:
    """A `TileServer` with a single GeoJSON source, `geojson_a`."""
    config = write_config(
        tmp_path,
        {
            "geojson": {
                "sources": {
                    "geojson_a": str(FIXTURES / "geojson" / "feature_collection_1.geojson"),
                }
            }
        },
    )
    return martin_py.TileServer(str(config))


@pytest.fixture
def multi_geojson_server(tmp_path: Path) -> martin_py.TileServer:
    """A `TileServer` with two GeoJSON sources, for composite (comma-joined) requests."""
    config = write_config(
        tmp_path,
        {
            "geojson": {
                "sources": {
                    "geojson_a": str(FIXTURES / "geojson" / "feature_collection_1.geojson"),
                    "geojson_b": str(FIXTURES / "geojson" / "feature_collection_2.geojson"),
                }
            }
        },
    )
    return martin_py.TileServer(str(config))


@pytest.fixture
def pmtiles_server(tmp_path: Path) -> martin_py.TileServer:
    """A `TileServer` with a single raster PMTiles source, `pmtiles_a`."""
    config = write_config(
        tmp_path,
        {
            "pmtiles": {
                "sources": {
                    "pmtiles_a": str(FIXTURES / "pmtiles" / "png.pmtiles"),
                }
            }
        },
    )
    return martin_py.TileServer(str(config))


@pytest.fixture
def bounded_zoom_server(tmp_path: Path) -> martin_py.TileServer:
    """A `TileServer` with a PMTiles source whose zoom range is limited (z0-z3)."""
    config = write_config(
        tmp_path,
        {
            "pmtiles": {
                "sources": {
                    "stamen_z3": str(
                        FIXTURES / "pmtiles" / "stamen_toner__raster_CC-BY+ODbL_z3.pmtiles"
                    ),
                }
            }
        },
    )
    return martin_py.TileServer(str(config))


@pytest.fixture
def duckdb_server(tmp_path: Path) -> martin_py.TileServer:
    """A `TileServer` with a single DuckDB/GeoParquet source, `polygons`.

    Same fixture and source shape as e2e-tests/tests/duckdb.rs's `POLYGONS` config.
    """
    config = write_config(
        tmp_path,
        {
            "duckdb": {
                "sources": [
                    {
                        "geoparquet": str(FIXTURES / "duckdb" / "geoparquet_polygons.parquet"),
                        "layer_id": "polygons",
                        "geometry_column": "geom",
                        "srid": 4326,
                        "minzoom": 0,
                        "maxzoom": 14,
                    }
                ]
            }
        },
    )
    return martin_py.TileServer(str(config))
