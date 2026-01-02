PRAGMA foreign_keys=OFF;
BEGIN TRANSACTION;
CREATE TABLE metadata (name text, value text);
INSERT INTO metadata VALUES
('name','Major cities from Natural Earth data'),
('description','A modified version of major cities from Natural Earth data'),
('version','2'),
('minzoom','0'),
('maxzoom','6'),
('center','-75.937500,38.788894,6'),
('bounds','-123.123590,-37.818085,174.763027,59.352706'),
('type','overlay'),
('format','pbf'),
('json','{"vector_layers": [ { "id": "cities", "description": "", "minzoom": 0, "maxzoom": 6, "fields": {"name": "String"} } ],"tilestats": {"layerCount": 1,"layers": [{"layer": "cities","count": 68,"geometry": "Point","attributeCount": 1,"attributes": [{"attribute": "name","count": 68,"type": "string","values": ["Addis Ababa","Amsterdam","Athens","Atlanta","Auckland","Baghdad","Bangalore","Bangkok","Beijing","Berlin","Bogota","Buenos Aires","Cairo","Cape Town","Caracas","Casablanca","Chengdu","Chicago","Dakar","Denver","Dubai","Geneva","Hong Kong","Houston","Istanbul","Jakarta","Johannesburg","Kabul","Kiev","Kinshasa","Kolkata","Lagos","Lima","London","Los Angeles","Madrid","Manila","Melbourne","Mexico City","Miami","Monterrey","Moscow","Mumbai","Nairobi","New Delhi","New York","Paris","Rio de Janeiro","Riyadh","Rome","San Francisco","Santiago","Seoul","Shanghai","Singapore","Stockholm","Sydney","São Paulo","Taipei","Tashkent","Tehran","Tokyo","Toronto","Vancouver","Vienna","Washington, D.C.","Ürümqi","Ōsaka"]}]}]}}'),
('agg_tiles_hash','578FB5BD64746C39E3D344662947FD0D');
CREATE TABLE tiles (zoom_level integer, tile_column integer, tile_row integer, tile_data blob);
INSERT INTO tiles VALUES
(1,0,0,x'1f8b080000000000000303000000000000000000'),
(2,3,1,x'1f8b0800000000000203939ac158c1c4c5969c5992995aacd1a020c59297989baac4cdc5199c99979e58905f94aac4c9c5ee95989d5854920812f74dcd49ca2f2dca4b55e2e0620bae4cc94bad54e2e2e2702c4dcece49cc4b11e29160146262605062e17cc3592ac40be6312ab172fee2dec402e53201b95dea5db2502e33907b40fb8a2494cb02e42eb299230300e9f2eb9a9b000000'),
(3,7,3,x'1f8b080000000000000303000000000000000000'),
(4,7,8,x'1f8b080000000000000303000000000000000000'),
(5,16,20,x'1f8b080000000000000303000000000000000000'),
(6,45,37,x'1f8b080000000000000303000000000000000000'),
(4,4,4,x'1f8b080000000000000303000000000000000000');
CREATE UNIQUE INDEX name ON metadata (name);
CREATE UNIQUE INDEX tile_index ON tiles (zoom_level, tile_column, tile_row);
COMMIT;
