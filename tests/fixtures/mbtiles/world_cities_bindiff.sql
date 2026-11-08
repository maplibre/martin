PRAGMA foreign_keys=OFF;
BEGIN TRANSACTION;
CREATE TABLE metadata (name text, value text);
INSERT INTO metadata VALUES
('description','A modified version of major cities from Natural Earth data'),
('agg_tiles_hash_before_apply','08934F8E3E58DED510920E1DE6F4E78F'),
('agg_tiles_hash_after_apply','72D8C992AF67EC97093B0087933FA160'),
('agg_tiles_hash','9FDA1E9908727A8C2ACB295210355E56'),
('hash_algorithm','md5');
CREATE TABLE tiles (zoom_level integer, tile_column integer, tile_row integer, tile_data blob);
INSERT INTO tiles VALUES
(0,0,0,NULL),
(4,4,4,x'1f8b080000000000000303000000000000000000');
CREATE TABLE bsdiffrawgz (
    zoom_level integer NOT NULL,
    tile_column integer NOT NULL,
    tile_row integer NOT NULL,
    patch_data blob NOT NULL,
    tile_xxh3_64_hash integer NOT NULL,
    PRIMARY KEY(zoom_level, tile_column, tile_row)
);
CREATE UNIQUE INDEX name ON metadata (name);
CREATE UNIQUE INDEX tile_index ON tiles (zoom_level, tile_column, tile_row);
COMMIT;
