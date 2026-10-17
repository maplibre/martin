"""Type stubs for the `martin_py` extension module."""

class TileServer:
    """
    An in-process handle onto the tile sources declared by a Martin config file.

    Loads and resolves all sources once, up front (just like the `martin` server
    does at startup), then serves tile requests directly against them without
    opening any network port.
    """

    def __init__(self, config_path: str) -> None:
        """Load and resolve every tile source declared in the Martin config file at `config_path`."""

    def list_sources(self) -> list[str]:
        """The ids of all currently registered tile sources (aliases are not included)."""

    def get_source_info(self, source_ids: str, tiles_url: str = "") -> str:
        """
        TileJSON metadata for one or more comma-joined source ids, as a JSON string.

        `source_ids` may combine several sources, e.g. "a,b", the same way the
        `/{source_ids}` HTTP endpoint does.
        """

    def get_tile(
        self,
        source_ids: str,
        z: int,
        x: int,
        y: int,
        query: str | None = None,
    ) -> tuple[bytes, str, str | None]:
        """
        Fetch one tile's raw, possibly-compressed bytes as they'd be sent over HTTP.

        Returns `(data, content_type, content_encoding)`; `content_encoding` is
        `None` when the tile is uncompressed.
        """
