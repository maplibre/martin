CREATE TABLE map (
    zoom_level INTEGER,
    tile_column INTEGER,
    tile_row INTEGER,
    tile_id TEXT
);

CREATE TABLE images (
    tile_id TEXT,
    tile_data BLOB
);

CREATE UNIQUE INDEX map_index ON map (
    zoom_level, tile_column, tile_row
);
CREATE UNIQUE INDEX images_id ON images (
    tile_id
);

CREATE VIEW tiles AS
SELECT
    map.zoom_level,
    map.tile_column,
    map.tile_row,
    images.tile_data
FROM
    map INNER JOIN images
    ON map.tile_id = images.tile_id;

CREATE VIEW tiles_with_hash AS
SELECT
    map.zoom_level,
    map.tile_column,
    map.tile_row,
    images.tile_data,
    images.tile_id AS tile_hash
FROM
    map INNER JOIN images
    ON map.tile_id = images.tile_id;
