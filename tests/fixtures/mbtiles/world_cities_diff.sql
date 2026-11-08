PRAGMA foreign_keys=OFF;
BEGIN TRANSACTION;
CREATE TABLE metadata (name text, value text);
INSERT INTO metadata VALUES
('description','A modified version of major cities from Natural Earth data'),
('agg_tiles_hash_before_apply','08934F8E3E58DED510920E1DE6F4E78F'),
('agg_tiles_hash_after_apply','72D8C992AF67EC97093B0087933FA160'),
('agg_tiles_hash','6A51FC6A9048BE033C08CDDEC0D028AC');
CREATE TABLE tiles (zoom_level integer, tile_column integer, tile_row integer, tile_data blob);
INSERT INTO tiles VALUES
(0,0,0,NULL),
(4,4,4,x'1f8b080000000000000303000000000000000000');
CREATE UNIQUE INDEX name ON metadata (name);
CREATE UNIQUE INDEX tile_index ON tiles (zoom_level, tile_column, tile_row);
COMMIT;
