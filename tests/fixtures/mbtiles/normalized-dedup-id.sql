PRAGMA foreign_keys=OFF;
BEGIN TRANSACTION;
CREATE TABLE tiles_shallow (
    zoom_level integer,
    tile_column integer,
    tile_row integer,
    tile_data_id integer,
    PRIMARY KEY(zoom_level, tile_column, tile_row)
) WITHOUT ROWID;
INSERT INTO tiles_shallow VALUES(0,0,0,1);
INSERT INTO tiles_shallow VALUES(1,0,0,2);
INSERT INTO tiles_shallow VALUES(1,0,1,3);
INSERT INTO tiles_shallow VALUES(1,1,0,4);
INSERT INTO tiles_shallow VALUES(1,1,1,5);
CREATE TABLE tiles_data (
    tile_data_id integer PRIMARY KEY,
    tile_data blob
);
INSERT INTO tiles_data VALUES(1,X'ffd8ffffFFD9');
INSERT INTO tiles_data VALUES(2,X'FFD8FF00D9');
INSERT INTO tiles_data VALUES(3,X'FFD8FFD9');
INSERT INTO tiles_data VALUES(4,X'ffd8ff00FFD9');
INSERT INTO tiles_data VALUES(5,X'FFD8FF11D9');
CREATE TABLE metadata (
    name text,
    value text
);
INSERT INTO metadata VALUES('bounds','-180,-85.0511,180,85.0511');
INSERT INTO metadata VALUES('minzoom','0');
INSERT INTO metadata VALUES('maxzoom','1');
INSERT INTO metadata VALUES('name','Normalized DedupId Test');
INSERT INTO metadata VALUES('description','Test fixture for normalized schema with integer tile_data_id');
INSERT INTO metadata VALUES('format','jpeg');
INSERT INTO metadata VALUES('agg_tiles_hash','3CE4DB27DDC5A385756CC384CDAFC3D5');
CREATE VIEW tiles AS
SELECT
    tiles_shallow.zoom_level,
    tiles_shallow.tile_column,
    tiles_shallow.tile_row,
    tiles_data.tile_data
FROM tiles_shallow
INNER JOIN tiles_data ON tiles_shallow.tile_data_id = tiles_data.tile_data_id;
CREATE UNIQUE INDEX name ON metadata (name);
COMMIT;
