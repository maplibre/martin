PRAGMA foreign_keys=OFF;
BEGIN TRANSACTION;
CREATE TABLE metadata (
    name text,
    value text
);
INSERT INTO metadata VALUES
('name','ne2sr'),
('format','webp'),
('type','baselayer'),
('bounds','-180,-85.05113,180,85.05113'),
('center','0,0,0'),
('minzoom','0'),
('maxzoom','0');
CREATE TABLE tiles (
    zoom_level integer,
    tile_column integer,
    tile_row integer,
    tile_data blob
);
INSERT INTO tiles VALUES(0,0,0,x'524946463A000000574542505650380A0000002F000000');
CREATE UNIQUE INDEX name ON metadata (name);
COMMIT;
