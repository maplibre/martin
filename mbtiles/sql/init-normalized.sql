CREATE TABLE map (
    zoom_level INTEGER NOT NULL,
    tile_column INTEGER NOT NULL,
    tile_row INTEGER NOT NULL,
    tile_id TEXT,
    PRIMARY KEY (zoom_level, tile_column, tile_row)
);

CREATE TABLE images (
    tile_id TEXT NOT NULL PRIMARY KEY,
    tile_data BLOB
);

CREATE VIEW tiles AS
SELECT
    map.zoom_level,
    map.tile_column,
    map.tile_row,
    images.tile_data
FROM map
INNER JOIN images ON map.tile_id = images.tile_id;
