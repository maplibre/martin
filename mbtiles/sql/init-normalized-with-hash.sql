CREATE VIEW tiles_with_hash AS
SELECT
    map.zoom_level AS zoom_level,
    map.tile_column AS tile_column,
    map.tile_row AS tile_row,
    images.tile_data AS tile_data,
    images.tile_id AS tile_hash
FROM map
JOIN images ON images.tile_id = map.tile_id;
