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
CREATE TABLE IF NOT EXISTS "tiles"(
  zoom_level INT,
  tile_column INT,
  tile_row INT,
  tile_data
);
INSERT INTO tiles VALUES(0,0,0,X'00');
CREATE UNIQUE INDEX name ON metadata (name);
COMMIT;
