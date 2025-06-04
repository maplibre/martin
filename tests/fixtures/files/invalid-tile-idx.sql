PRAGMA foreign_keys=OFF;
BEGIN TRANSACTION;
CREATE TABLE metadata (name text NOT NULL PRIMARY KEY, value text);
INSERT INTO metadata VALUES
('name','Major cities from Natural Earth data'),
('description','Major cities from Natural Earth data'),
('version','2'),
('minzoom','0'),
('maxzoom','6'),
('center','-75.937500,38.788894,6'),
('bounds','-123.123590,-37.818085,174.763027,59.352706'),
('type','overlay'),
('format','pbf'),
('json','{"vector_layers": [ { "id": "cities", "description": "", "minzoom": 0, "maxzoom": 6, "fields": {"name": "String"} } ],"tilestats": {"layerCount": 1,"layers": [{"layer": "cities","count": 68,"geometry": "Point","attributeCount": 1,"attributes": [{"attribute": "name","count": 68,"type": "string","values": ["Addis Ababa","Amsterdam","Athens","Atlanta","Auckland","Baghdad","Bangalore","Bangkok","Beijing","Berlin","Bogota","Buenos Aires","Cairo","Cape Town","Caracas","Casablanca","Chengdu","Chicago","Dakar","Denver","Dubai","Geneva","Hong Kong","Houston","Istanbul","Jakarta","Johannesburg","Kabul","Kiev","Kinshasa","Kolkata","Lagos","Lima","London","Los Angeles","Madrid","Manila","Melbourne","Mexico City","Miami","Monterrey","Moscow","Mumbai","Nairobi","New Delhi","New York","Paris","Rio de Janeiro","Riyadh","Rome","San Francisco","Santiago","Seoul","Shanghai","Singapore","Stockholm","Sydney","São Paulo","Taipei","Tashkent","Tehran","Tokyo","Toronto","Vancouver","Vienna","Washington, D.C.","Ürümqi","Ōsaka"]}]}]}}'),
('agg_tiles_hash','D4E1030D57751A0B45A28A71267E46B8');
CREATE TABLE tiles_with_hash (zoom_level integer NOT NULL, tile_column integer NOT NULL, tile_row integer NOT NULL, tile_data blob, tile_hash text,
                PRIMARY KEY(zoom_level, tile_column, tile_row));
INSERT INTO tiles_with_hash VALUES(6,10,64,X'1f8b08000000000002039332ae60e2624bce2cc94c2dd668509062c94bcc4d55e2e7e20d4ecc53702b4acc4bce2c4ece17e29560146262605062e59cc2bf481f004d6b26c735000000','53B658F89AF42BD1D9C21C14BF006E04');
CREATE VIEW tiles AS SELECT zoom_level, tile_column, tile_row, tile_data FROM tiles_with_hash;
COMMIT;
