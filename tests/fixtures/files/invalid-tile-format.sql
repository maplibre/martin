PRAGMA foreign_keys=OFF;
BEGIN TRANSACTION;
CREATE TABLE map (
    zoom_level INTEGER,
    tile_column INTEGER,
    tile_row INTEGER,
    tile_id TEXT,
    grid_id TEXT
);
INSERT INTO map VALUES(0,0,0,'b0a559d767b45de037d4aefe483d3a274dff23e8',NULL),
(1,0,0,'00594FD4F42BA43FC1CA0427A0576295',NULL);
CREATE TABLE keymap (
    key_name TEXT,
    key_json TEXT
);
CREATE TABLE images (
    tile_data BLOB,
    tile_id TEXT
);
INSERT INTO images VALUES(X'ff','00594FD4F42BA43FC1CA0427A0576295');
CREATE TABLE metadata (
    name TEXT,
    value TEXT
);
INSERT INTO metadata VALUES
('tilejson','2.0.0'),
('version','1.0.0'),
('name','test'),
('description','test'),
('attribution','test'),
('minzoom','0'),
('maxzoom','1'),
('bounds','-81.639308,28.618110,-70.816397,38.362041'),
('center','-76.227852,33.490075,4');
CREATE VIEW tiles AS
SELECT
    map.zoom_level,
    map.tile_column,
    map.tile_row,
    images.tile_data
FROM map
INNER JOIN images ON map.tile_id = images.tile_id;
CREATE UNIQUE INDEX map_index ON map (zoom_level, tile_column, tile_row);
CREATE UNIQUE INDEX keymap_lookup ON keymap (key_name);
CREATE UNIQUE INDEX images_id ON images (tile_id);
CREATE UNIQUE INDEX name ON metadata (name);
COMMIT;
