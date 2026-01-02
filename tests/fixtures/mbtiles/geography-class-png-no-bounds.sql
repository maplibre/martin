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
(0,0,0,'CDEE5DAAC3EBDC5180E5148B63992309','a592787e1b98714c9af7ba3e494166db'),
(1,0,0,'F274F66CEF892C60179A8AC491138FFB','f4e6039f6261ecdf5a9ca153121e5ad7'),
(1,0,1,'577B2577884E5415204AA437735B94E3','38119e84848bfb161d4d81e07d241b58'),
(1,1,0,'DD4FFC9BC0136A61C780B7AB7E222CB9','57a5641e4893608878e715fd628870cd'),
(1,1,1,'95EB7C26C4D4854C0291F5470BB2035D','710f5a40afdc3155cf458ebcfdd76c09');
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
(X'89504E470D0A1A0A01','CDEE5DAAC3EBDC5180E5148B63992309'),
(X'89504E470D0A1A0A02','F274F66CEF892C60179A8AC491138FFB'),
(X'89504E470D0A1A0A03','577B2577884E5415204AA437735B94E3'),
(X'89504E470D0A1A0A04','DD4FFC9BC0136A61C780B7AB7E222CB9'),
(X'89504E470D0A1A0A05','95EB7C26C4D4854C0291F5470BB2035D');
CREATE TABLE metadata (
    name TEXT,
    value TEXT
);
INSERT INTO metadata VALUES
('minzoom','0'),
('maxzoom','1'),
('legend','<div style="text-align:center;">' || X'0A0A' || '<div style="font:12pt/16pt Georgia,serif;">Geography Class</div>' || X'0A' || '<div style="font:italic 10pt/16pt Georgia,serif;">by MapBox</div>' || X'0A0A' || '<img src="data:image/png;base64,iVBORw0KGgo">' || X'0A' || '</div>'),
('name','Geography Class'),
('description','One of the example maps that comes with TileMill - a bright & colorful world map that blends retro and high-tech with its folded paper texture and interactive flag tooltips. '),
('attribution',''),
('template', ''),
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
