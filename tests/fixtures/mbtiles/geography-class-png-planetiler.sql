PRAGMA foreign_keys=OFF;
BEGIN TRANSACTION;
CREATE TABLE metadata (name text, value text);
INSERT INTO metadata VALUES
('minzoom','0'),
('maxzoom','1'),
('name','Geography Class'),
('description','One of the example maps that comes with TileMill - a bright & colorful world map that blends retro and high-tech with its folded paper texture and interactive flag tooltips. '),
('version','1.0.0');
CREATE UNIQUE INDEX name ON metadata (name);
CREATE TABLE tiles_shallow (
    zoom_level integer,
    tile_column integer,
    tile_row integer,
    tile_data_id integer,
    PRIMARY KEY(zoom_level,tile_column,tile_row)
) WITHOUT ROWID;
INSERT INTO tiles_shallow VALUES
(0,0,0,1),
(1,0,0,2),
(1,0,1,3),
(1,1,0,4),
(1,1,1,5);
CREATE TABLE tiles_data (
    tile_data_id integer PRIMARY KEY,
    tile_data blob
);
INSERT INTO tiles_data VALUES
(1,X'89504E470D0A1A0A01'),
(2,X'89504E470D0A1A0A02'),
(3,X'89504E470D0A1A0A03'),
(4,X'89504E470D0A1A0A04'),
(5,X'89504E470D0A1A0A05');
CREATE VIEW tiles AS
SELECT
    tiles_shallow.zoom_level,
    tiles_shallow.tile_column,
    tiles_shallow.tile_row,
    tiles_data.tile_data
FROM tiles_shallow
INNER JOIN tiles_data ON tiles_shallow.tile_data_id = tiles_data.tile_data_id;
/* tiles(zoom_level,tile_column,tile_row,tile_data) */
COMMIT;
