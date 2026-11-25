CREATE TABLE tiles (
    zoom_level INTEGER NOT NULL,
    tile_column INTEGER NOT NULL,
    tile_row INTEGER NOT NULL,
    tile_data BLOB,
    PRIMARY KEY (zoom_level, tile_column, tile_row)
);
