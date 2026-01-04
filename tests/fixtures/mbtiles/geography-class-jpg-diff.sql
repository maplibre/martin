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
(0,0,0,NULL,NULL),
(1,1,1,'4F611D47B1BD6F9BE94DB7D713053A5E',NULL),
(2,2,2,'E65F5FDBC963CBE2E6112C787C9B7BBD',NULL);
CREATE TABLE images (
    tile_data BLOB,
    tile_id TEXT
);
INSERT INTO images VALUES
(NULL,NULL),
(X'ffd8ff22d9','E2532D4D5EBE5A71437F428D4141857C'),
(X'ffd8ffffffd9','E65F5FDBC963CBE2E6112C787C9B7BBD');
CREATE TABLE metadata (
    name TEXT,
    value TEXT
);
INSERT INTO metadata VALUES
('bounds','-180,-85.0511,180,85.0511'),
('minzoom','1'),
('maxzoom','2'),
('legend','<div style="text-align:center;">' || X'0A0A' || '<div style="font:12pt/16pt Georgia,serif;">Geography Class</div>' || X'0A' || '<div style="font:italic 10pt/16pt Georgia,serif;">by MapBox</div>' || X'0A0A' || '<img src="data:image/png;base64,iVBORw0KGgo">' || X'0A' || '</div>'),
('name','Geography Class'),
('attribution',''),
('template','foobar'),
('description','A modified version of one of the example maps that comes with TileMill - a bright & colorful world map that blends retro and high-tech with its folded paper texture and interactive flag tooltips.'),
('agg_tiles_hash_before_apply','3CE4DB27DDC5A385756CC384CDAFC3D5'),
('agg_tiles_hash_after_apply','0B5497687F57C65097D89B675E1AC255'),
('agg_tiles_hash','3B8D2E670D8AB80705357D3662506952'),
('version','1.0.0');
CREATE VIEW tiles AS
SELECT
    map.zoom_level,
    map.tile_column,
    map.tile_row,
    images.tile_data
FROM map
INNER JOIN images ON map.tile_id = images.tile_id;
CREATE UNIQUE INDEX map_index ON map (zoom_level, tile_column, tile_row);
CREATE UNIQUE INDEX images_id ON images (tile_id);
CREATE UNIQUE INDEX name ON metadata (name);
COMMIT;
