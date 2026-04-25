CREATE TABLE tiles (
    zoom_level INTEGER,
    tile_column INTEGER,
    tile_row INTEGER,
    tile_data BLOB
);

CREATE UNIQUE INDEX tile_index ON tiles (
    zoom_level, tile_column, tile_row
);
