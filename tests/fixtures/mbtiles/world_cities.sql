PRAGMA foreign_keys=OFF;
BEGIN TRANSACTION;
CREATE TABLE metadata (name text, value text);
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
('agg_tiles_hash','84792BF4EE9AEDDC5B1A60E707011FEE');
CREATE TABLE tiles (zoom_level integer, tile_column integer, tile_row integer, tile_data blob);
INSERT INTO tiles VALUES
(0,0,0,x'1f8b080000000000000303000000000000000000'),
(1,0,0,x'1f8b080000000000000303000000000000000000'),
(2,3,1,x'1f8b080000000000000303000000000000000000'),
(3,7,3,x'1f8b080000000000000303000000000000000000'),
(4,7,8,x'1f8b080000000000000303000000000000000000'),
(5,16,20,x'1f8b080000000000000303000000000000000000'),
(6,45,37,x'1f8b080000000000000303000000000000000000');
CREATE UNIQUE INDEX name ON metadata (name);
CREATE UNIQUE INDEX tile_index ON tiles (zoom_level, tile_column, tile_row);
COMMIT;
