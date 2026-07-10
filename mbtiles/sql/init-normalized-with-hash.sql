CREATE VIEW tiles_with_hash AS
SELECT
    map.zoom_level,
    map.tile_column,
    map.tile_row,
    images.tile_data,
    images.tile_id AS tile_hash
FROM map
INNER JOIN images ON map.tile_id = images.tile_id;
