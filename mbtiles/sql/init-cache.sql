CREATE TABLE tile_cache (
    zoom_level INTEGER NOT NULL,
    tile_column INTEGER NOT NULL,
    tile_row INTEGER NOT NULL,
    fetched INTEGER,
    expires INTEGER,
    etag TEXT,
    tile_data BLOB NOT NULL,
    PRIMARY KEY (zoom_level, tile_column, tile_row)
) WITHOUT ROWID;

CREATE VIEW tiles AS
SELECT
    zoom_level,
    tile_column,
    tile_row,
    tile_data
FROM tile_cache;
