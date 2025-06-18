PRAGMA foreign_keys=OFF;
BEGIN TRANSACTION;
.read mbtiles/sql/init-flat.sql
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
('json','{"vector_layers": [ { "id": "cities", "description": "", "minzoom": 0, "maxzoom": 6, "fields": {"name": "String"} } ],"tilestats": {"layerCount": 1,"layers": [{"layer": "cities","count": 68,"geometry": "Point","attributeCount": 1,"attributes": [{"attribute": "name","count": 68,"type": "string","values": ["Addis Ababa","Amsterdam","Athens","Atlanta","Auckland","Baghdad","Bangalore","Bangkok","Beijing","Berlin","Bogota","Buenos Aires","Cairo","Cape Town","Caracas","Casablanca","Chengdu","Chicago","Dakar","Denver","Dubai","Geneva","Hong Kong","Houston","Istanbul","Jakarta","Johannesburg","Kabul","Kiev","Kinshasa","Kolkata","Lagos","Lima","London","Los Angeles","Madrid","Manila","Melbourne","Mexico City","Miami","Monterrey","Moscow","Mumbai","Nairobi","New Delhi","New York","Paris","Rio de Janeiro","Riyadh","Rome","San Francisco","Santiago","Seoul","Shanghai","Singapore","Stockholm","Sydney","São Paulo","Taipei","Tashkent","Tehran","Tokyo","Toronto","Vancouver","Vienna","Washington, D.C.","Ürümqi","Ōsaka"]}]}]}}');
INSERT INTO tiles VALUES
(0,0,0,NULL),
(1,1,1,X'1f8b080000000000020335cf3f6813511c07f026bdfcb9bb44639ac478228d8ffa1744aa5011a1985810495ba416f777b947ee91bbf7f0eed2ea260ecea5833864281d1c1c9c3a3888838592d141a4931407c9548a7470f4f70bbf4cf7fbdc7deffb7bcf39c8bc4c5bd98e4ca488afbf6e3886e2a160792bbbac95a715b32df359a23b3d5f0721a219c689883c1eb29c9579ca231963f8b1506283e3d4125120154ecfa5508ab3ac65ac69a884675b8a0d6659f92771c295db0f30b5a2e38edec4b265ded5e3b266e20b15e3ab475c469a9956aec5bbbec73d56b4eca6e7c9b8d174b9cbb16b9dc77e4fa804e7fd41b4ff257c21b1645df8111f9f634dbee29e8f754b7d974b1cda1cb7c36d56c566634904bec4256d1df47832bec54a3fc428245a5c7579a0238189553c8f2bcb857aaa9c9e9a6286696f55cbc5b1522c631e17766c621af8757abb429c06fe48bdad130de027e3d8216680dfcc7f33c42cf0bbfd7b12ce014ff25bb3c43cf0b0fab14634315c39997cb580bf9c3725a20ddc4bef3d2016b0aa74c48845e0a036bc413c031cce8eae12cfe2a92e1edc279680bbf33f278bce2117772f11cbf8efdc688e3803dcb97c7a8b58017eb8f679c22af0fdfce80ab106fcbb707493781ef86771789b5847de1ddc215e006edf3b5c203ac077cee9c3ff460d3b17c9020000'),
(4,4,4,X'1f8b08000000000000ff33a83031020022bc70f804000000');
CREATE UNIQUE INDEX name on metadata (name);
COMMIT;
