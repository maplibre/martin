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
(2,2,2,'E65F5FDBC963CBE2E6112C787C9B7BBD','a592787e1b98714c9af7ba3e494166db'),
(1,0,0,'DB4D3CBC1929A6321AA9375F4BADC02E','f4e6039f6261ecdf5a9ca153121e5ad7'),
(1,0,1,'F5EE0EC808CB0DA7058C84210B3FBB87','38119e84848bfb161d4d81e07d241b58'),
(1,1,0,'FF76F8F69FC9E242711C71EA6E4CF200','57a5641e4893608878e715fd628870cd'),
(1,1,1,'E2532D4D5EBE5A71437F428D4141857C','710f5a40afdc3155cf458ebcfdd76c09');
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
(X'ffd8ffffFFD9','E65F5FDBC963CBE2E6112C787C9B7BBD'),
(X'FFD8FF00D9','DB4D3CBC1929A6321AA9375F4BADC02E'),
(X'FFD8FFD9','F5EE0EC808CB0DA7058C84210B3FBB87'),
(X'ffd8ff00FFD9','FF76F8F69FC9E242711C71EA6E4CF200'),
(X'FFD8FF22D9','E2532D4D5EBE5A71437F428D4141857C');
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
