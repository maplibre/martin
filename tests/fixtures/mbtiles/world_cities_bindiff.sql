PRAGMA foreign_keys=OFF;
BEGIN TRANSACTION;
CREATE TABLE metadata (name text, value text);
INSERT INTO metadata VALUES
('description','A modified version of major cities from Natural Earth data'),
('agg_tiles_hash_before_apply','84792BF4EE9AEDDC5B1A60E707011FEE'),
('agg_tiles_hash_after_apply','578FB5BD64746C39E3D344662947FD0D'),
('agg_tiles_hash','0A21AAF2C177B86DA3342A4F65794E49');
CREATE TABLE tiles (zoom_level integer, tile_column integer, tile_row integer, tile_data blob);
INSERT INTO tiles VALUES
(0,0,0,NULL),
(4,4,4,X'1f8b08000000000000ff33a83031020022bc70f804000000');
CREATE TABLE bsdiffrawgz (
             zoom_level integer NOT NULL,
             tile_column integer NOT NULL,
             tile_row integer NOT NULL,
             patch_data blob NOT NULL,
             tile_xxh3_64_hash integer NOT NULL,
             PRIMARY KEY(zoom_level, tile_column, tile_row));
INSERT INTO bsdiffrawgz VALUES(1,1,1,X'1b1003f81f05ee9e8322940c625cb86d4bdb2b02626ebf0d889ab8653c99246a233542266593eba6119fe766bf6c78426b6222a1c61c4eea8563ccbbb70d454594a4a29115b8d6ab45a1eaba0d1500082c701d00002600f7aff01890029084a4525e0b8081f805004e4837080024a99870b7430009003300000a68f500e0c25f038097281d0018903701e0e76c0b00aa764d006834b2020031247f000c549a01409ac97600dcbab602804b643f00ecb39b0090af700440716a0b00d88ee704c071a5270096b9ad00185d6907009dca2e00fc4fdc0060feb817308d371b',-6472923538033265914);
CREATE UNIQUE INDEX name on metadata (name);
CREATE UNIQUE INDEX tile_index on tiles (zoom_level, tile_column, tile_row);
COMMIT;
