PRAGMA foreign_keys=OFF;
BEGIN TRANSACTION;
CREATE TABLE map (
    zoom_level INTEGER,
    tile_column INTEGER,
    tile_row INTEGER,
    tile_id TEXT,
    grid_id TEXT
);
INSERT INTO map VALUES
(0,0,0,'',NULL),
(1,1,0,'d8018fba714e93c29500adb778b587a5',NULL),
(1,1,1,'d8018fba714e93c29500adb778b587a5',NULL),
(2,2,2,'035e1077aab736ad34208aaea571d6ac',NULL);
CREATE TABLE images (
    tile_data BLOB,
    tile_id TEXT
);
INSERT INTO images VALUES
(NULL,''),
(X'ffd80000ffd9','d8018fba714e93c29500adb778b587a5'),
(X'ffd80000ffd9','035e1077aab736ad34208aaea571d6ac');
CREATE TABLE metadata (
    name TEXT,
    value TEXT
);
INSERT INTO metadata VALUES
('description','A modified version of one of the example maps that comes with TileMill - a bright & colorful world map that blends retro and high-tech with its folded paper texture and interactive flag tooltips.'),
('agg_tiles_hash','ED6EE825AFB8F85F2CC53ECDF53539A8');
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
    map.zoom_level,
    map.tile_column,
    map.tile_row,
    images.tile_data,
    images.tile_id AS tile_hash
FROM map
INNER JOIN images ON map.tile_id = images.tile_id;
CREATE UNIQUE INDEX map_index ON map (zoom_level, tile_column, tile_row);
CREATE UNIQUE INDEX images_id ON images (tile_id);
CREATE UNIQUE INDEX name ON metadata (name);
COMMIT;
