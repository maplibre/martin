CREATE TABLE cache_data (
    tile_id INTEGER NOT NULL PRIMARY KEY,
    tile_data BLOB NOT NULL
);

CREATE TABLE tile_cache (
    zoom_level INTEGER NOT NULL,
    tile_column INTEGER NOT NULL,
    tile_row INTEGER NOT NULL,
    expires INTEGER,
    etag TEXT,
    tile_id INTEGER NOT NULL,
    PRIMARY KEY (zoom_level, tile_column, tile_row)
) WITHOUT ROWID;

CREATE VIEW tiles AS
SELECT
    tile_cache.zoom_level,
    tile_cache.tile_column,
    tile_cache.tile_row,
    cache_data.tile_data
FROM
    tile_cache INNER JOIN cache_data
    ON tile_cache.tile_id = cache_data.tile_id;
