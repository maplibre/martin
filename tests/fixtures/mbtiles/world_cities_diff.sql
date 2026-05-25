PRAGMA foreign_keys=OFF;
BEGIN TRANSACTION;
CREATE TABLE metadata (name text, value text);
INSERT INTO metadata VALUES
('description','A modified version of major cities from Natural Earth data'),
('agg_tiles_hash_before_apply','84792BF4EE9AEDDC5B1A60E707011FEE'),
('agg_tiles_hash_after_apply','578FB5BD64746C39E3D344662947FD0D'),
('agg_tiles_hash','6A51FC6A9048BE033C08CDDEC0D028AC');
CREATE TABLE tiles (zoom_level integer, tile_column integer, tile_row integer, tile_data blob);
INSERT INTO tiles VALUES
(0,0,0,NULL),
(4,4,4,x'1f8b080000000000000303000000000000000000');
CREATE UNIQUE INDEX name ON metadata (name);
CREATE UNIQUE INDEX tile_index ON tiles (zoom_level, tile_column, tile_row);
COMMIT;
