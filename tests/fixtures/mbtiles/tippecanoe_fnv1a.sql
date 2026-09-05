-- A normalized file the way tippecanoe writes one: tile_id is the 64-bit FNV-1a of the tile as decimal digits, and no hash_algorithm key.
CREATE TABLE map (zoom_level INTEGER, tile_column INTEGER, tile_row INTEGER, tile_id TEXT);
CREATE TABLE images (zoom_level INTEGER, tile_data BLOB, tile_id TEXT);
CREATE UNIQUE INDEX map_index ON map (zoom_level, tile_column, tile_row);
CREATE UNIQUE INDEX images_id ON images (zoom_level, tile_id);
CREATE VIEW tiles AS SELECT
    map.zoom_level,
    map.tile_column,
    map.tile_row,
    images.tile_data
FROM map INNER JOIN images ON map.tile_id = images.tile_id AND map.zoom_level = images.zoom_level;
CREATE TABLE metadata (name TEXT, value TEXT);
INSERT INTO metadata VALUES ('name', 'tippecanoe-style'), ('format', 'pbf'), ('minzoom', '0'), ('maxzoom', '1'), ('bounds', '-180,-85,180,85');
INSERT INTO images (zoom_level, tile_data, tile_id) VALUES (0, X'74696C652D303030', '806997324184764596');
INSERT INTO map VALUES (0, 0, 0, '806997324184764596');
INSERT INTO images (zoom_level, tile_data, tile_id) VALUES (1, X'74696C652D313030', '1461737806916054559');
INSERT INTO map VALUES (1, 0, 0, '1461737806916054559');
INSERT INTO map VALUES (1, 1, 0, '1461737806916054559');
INSERT INTO images (zoom_level, tile_data, tile_id) VALUES (1, X'74696C652D313031', '1461736707404426348');
INSERT INTO map VALUES (1, 0, 1, '1461736707404426348');
