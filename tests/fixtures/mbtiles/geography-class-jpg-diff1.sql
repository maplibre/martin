PRAGMA foreign_keys=OFF;
BEGIN TRANSACTION;
CREATE TABLE map (
    zoom_level INTEGER,
    tile_column INTEGER,
    tile_row INTEGER,
    tile_id TEXT,
    grid_id TEXT
);
INSERT INTO map VALUES(0,0,0,'',NULL);
INSERT INTO map VALUES(1,1,1,'E2532D4D5EBE5A71437F428D4141857C',NULL);
INSERT INTO map VALUES(2,2,2,'E65F5FDBC963CBE2E6112C787C9B7BBD',NULL);
CREATE TABLE images (
    tile_data BLOB,
    tile_id TEXT
);
INSERT INTO images VALUES(NULL,'');
INSERT INTO images VALUES(X'ffd8ff22d9','E2532D4D5EBE5A71437F428D4141857C');
INSERT INTO images VALUES(X'ffd8ffffffd9','E65F5FDBC963CBE2E6112C787C9B7BBD');
CREATE TABLE metadata (
    name TEXT,
    value TEXT
);
INSERT INTO metadata VALUES('minzoom','1');
INSERT INTO metadata VALUES('maxzoom','2');
INSERT INTO metadata VALUES('description','A modified version of one of the example maps that comes with TileMill - a bright & colorful world map that blends retro and high-tech with its folded paper texture and interactive flag tooltips.');
INSERT INTO metadata VALUES('agg_tiles_hash_before_apply','3CE4DB27DDC5A385756CC384CDAFC3D5');
INSERT INTO metadata VALUES('agg_tiles_hash_after_apply','0B5497687F57C65097D89B675E1AC255');
INSERT INTO metadata VALUES('agg_tiles_hash','3B8D2E670D8AB80705357D3662506952');
CREATE VIEW tiles AS
SELECT
    map.zoom_level,
    map.tile_column,
    map.tile_row,
    images.tile_data
FROM map
INNER JOIN images ON map.tile_id = images.tile_id;
CREATE VIEW tiles_with_hash AS
             SELECT
                 map.zoom_level AS zoom_level,
                 map.tile_column AS tile_column,
                 map.tile_row AS tile_row,
                 images.tile_data AS tile_data,
                 images.tile_id AS tile_hash
             FROM map
             JOIN images ON images.tile_id = map.tile_id;
CREATE UNIQUE INDEX map_index ON map (zoom_level, tile_column, tile_row);
CREATE UNIQUE INDEX images_id ON images (tile_id);
CREATE UNIQUE INDEX name ON metadata (name);
COMMIT;
