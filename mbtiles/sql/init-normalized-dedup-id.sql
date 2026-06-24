CREATE TABLE metadata (
    name TEXT,
    value TEXT
);

CREATE UNIQUE INDEX name ON metadata (
    name
);

CREATE TABLE tiles_data (
    tile_data_id INTEGER PRIMARY KEY,
    tile_data BLOB
);

CREATE TABLE tiles_shallow (
    zoom_level INTEGER,
    tile_column INTEGER,
    tile_row INTEGER,
    tile_data_id INTEGER,
    PRIMARY KEY (zoom_level, tile_column, tile_row)
) WITHOUT ROWID;

CREATE VIEW tiles AS
SELECT
    tiles_shallow.zoom_level,
    tiles_shallow.tile_column,
    tiles_shallow.tile_row,
    tiles_data.tile_data
FROM
    tiles_shallow
INNER JOIN tiles_data
    ON tiles_shallow.tile_data_id = tiles_data.tile_data_id;
