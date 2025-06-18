PRAGMA foreign_keys=OFF;
BEGIN TRANSACTION;
.read mbtiles/sql/init-flat.sql
CREATE TABLE metadata (name text, value text);
INSERT INTO metadata VALUES
('name','Dummy json data'),
('version','2'),
('minzoom','0'),
('maxzoom','0'),
('center','-75.937500,38.788894,6'),
('bounds','-123.123590,-37.818085,174.763027,59.352706'),
('format','json');
INSERT INTO tiles VALUES(0,0,0,X'7b22666f6f223a22626172227d');
CREATE UNIQUE INDEX name on metadata (name);
COMMIT;
