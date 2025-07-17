CREATE TABLE tiles_with_hash (
    zoom_level INTEGER NOT NULL,
    tile_column INTEGER NOT NULL,
    tile_row INTEGER NOT NULL,
    tile_data BLOB,
    tile_hash TEXT
);

CREATE UNIQUE INDEX tiles_with_hash_index ON tiles_with_hash (
    zoom_level, tile_column, tile_row
);

CREATE VIEW tiles AS
SELECT
    zoom_level,
    tile_column,
    tile_row,
    tile_data
FROM tiles_with_hash;
