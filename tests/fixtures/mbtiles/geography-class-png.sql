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
(0,0,0,'1578fdca522831a6435f7795586c235b','a592787e1b98714c9af7ba3e494166db'),
(1,0,0,'ae0ac83504c9b8c59447d628c929a50f','f4e6039f6261ecdf5a9ca153121e5ad7'),
(1,0,1,'0335e33fad4e219d8d0bb36f34746a91','38119e84848bfb161d4d81e07d241b58'),
(1,1,0,'62c29e1510c08b974879d7eae28469e7','57a5641e4893608878e715fd628870cd'),
(1,1,1,'8dffe8763c6fdb018f24e54e5bba2755','710f5a40afdc3155cf458ebcfdd76c09');
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
('710f5a40afdc3155cf458ebcfdd76c09',X'789C03');
CREATE TABLE images (
    tile_data BLOB,
    tile_id TEXT
);
INSERT INTO images VALUES
(X'89504E470D0A1A0A','1578fdca522831a6435f7795586c235b'),
(X'89504E470D0A1A0A','ae0ac83504c9b8c59447d628c929a50f'),
(X'89504E470D0A1A0A','0335e33fad4e219d8d0bb36f34746a91'),
(X'89504E470D0A1A0A','62c29e1510c08b974879d7eae28469e7'),
(X'89504E470D0A1A0A','8dffe8763c6fdb018f24e54e5bba2755');
CREATE TABLE metadata (
    name TEXT,
    value TEXT
);
INSERT INTO metadata VALUES
('bounds','-180,-85.0511,180,85.0511'),
('center','0,20,0'),
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
