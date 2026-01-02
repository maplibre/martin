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
(2,2,2,'035e1077aab736ad34208aaea571d6ac','a592787e1b98714c9af7ba3e494166db'),
(1,0,0,'f7cb51a3403b156551bfa77023c81f8a','f4e6039f6261ecdf5a9ca153121e5ad7'),
(1,0,1,'58e516125a2c9009f094ca995b06425c','38119e84848bfb161d4d81e07d241b58'),
(1,1,0,'d8018fba714e93c29500adb778b587a5','57a5641e4893608878e715fd628870cd'),
(1,1,1,'d8018fba714e93c29500adb778b587a5','710f5a40afdc3155cf458ebcfdd76c09');
CREATE TABLE grid_key (
    grid_id TEXT,
    key_name TEXT
);
INSERT INTO grid_key VALUES
('a592787e1b98714c9af7ba3e494166db','3'),
('710f5a40afdc3155cf458ebcfdd76c09','3');
CREATE TABLE keymap (
    key_name TEXT,
    key_json TEXT
);
INSERT INTO keymap VALUES
('3','{"admin":"Afghanistan","flag_png":"iVBORw0KGgo"}');
CREATE TABLE grid_utfgrid (
    grid_id TEXT,
    grid_utfgrid BLOB
);
INSERT INTO grid_utfgrid VALUES
('a592787e1b98714c9af7ba3e494166db',X'789C03'),
('38119e84848bfb161d4d81e07d241b58',X'789C03'),
('f4e6039f6261ecdf5a9ca153121e5ad7',X'789C03'),
('57a5641e4893608878e715fd628870cd',X'789C03'),
('710f5a40afdc3155cf458ebcfdd76c09',X'789C03');
CREATE TABLE images (
    tile_data BLOB,
    tile_id TEXT
);
INSERT INTO images VALUES
(X'ffd8ff00FFD9','035e1077aab736ad34208aaea571d6ac'),
(X'FFD8FFD9','f7cb51a3403b156551bfa77023c81f8a'),
(X'FFD8FFD9','58e516125a2c9009f094ca995b06425c'),
(X'ffd8ff00FFD9','d8018fba714e93c29500adb778b587a5');
CREATE TABLE metadata (
    name TEXT,
    value TEXT
);
INSERT INTO metadata VALUES
('bounds','-180,-85.0511,180,85.0511'),
('minzoom','0'),
('maxzoom','1'),
('legend','<div style="text-align:center;">' || X'0A0A' || '<div style="font:12pt/16pt Georgia,serif;">Geography Class</div>' || X'0A' || '<div style="font:italic 10pt/16pt Georgia,serif;">by MapBox</div>' || X'0A0A' || '<img src="data:image/png;base64,iVBORw0KGgo">' || X'0A' || '</div>'),
('name','Geography Class'),
('description','A modified version of one of the example maps that comes with TileMill - a bright & colorful world map that blends retro and high-tech with its folded paper texture and interactive flag tooltips.'),
('attribution',''),
('template', 'foobar'),
('version','1.0.0');
CREATE VIEW tiles AS
SELECT
    map.zoom_level,
    map.tile_column,
    map.tile_row,
    images.tile_data
FROM map
INNER JOIN images ON map.tile_id = images.tile_id;
CREATE VIEW grids AS
SELECT
    map.zoom_level,
    map.tile_column,
    map.tile_row,
    grid_utfgrid.grid_utfgrid AS grid
FROM map
INNER JOIN grid_utfgrid ON map.grid_id = grid_utfgrid.grid_id;
CREATE VIEW grid_data AS
SELECT
    map.zoom_level,
    map.tile_column,
    map.tile_row,
    keymap.key_name,
    keymap.key_json
FROM map
INNER JOIN grid_key ON map.grid_id = grid_key.grid_id
INNER JOIN keymap ON grid_key.key_name = keymap.key_name;
CREATE UNIQUE INDEX map_index ON map (zoom_level, tile_column, tile_row);
CREATE UNIQUE INDEX grid_key_lookup ON grid_key (grid_id, key_name);
CREATE UNIQUE INDEX keymap_lookup ON keymap (key_name);
CREATE UNIQUE INDEX grid_utfgrid_lookup ON grid_utfgrid (grid_id);
CREATE UNIQUE INDEX images_id ON images (tile_id);
CREATE UNIQUE INDEX name ON metadata (name);
COMMIT;
