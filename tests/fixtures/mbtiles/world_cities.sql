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
(0,0,0,X'1f8b080000000000020335935d481c5714c78df573d475dd6834c6e8387eb77e345653937ed8dd9536f5230495943ededdb9ecdceeecbd763e34bef950a4140925842241426b42902221482825f850300d12421ff210444a1e448a94508a84d28790f69c71ce8bdedfdc7bfee79cff395bff43e44abe5694169ee06ee7825e5f20598e1b655ae96526d3ca9fe58e51a9554c31a97fecc017e1a69551a1958d2b578fcb0cb7b96b946845235ce24b089b50d2e38ec3e78d52adf882f25d4f490c98e057445ae949e10537494ba45946e1715a3910131ce39ecda4c78c62ad7042b09c30625af433e65a426640a65b1fe94df61a9a567291cfe99f2b278ba9132aa320043599c3d22ca8675c4913f2966b5a92b92c05aa6986df2798e90813f547589639469156302e720c35a1454f604911ad3ce173890d0a07da0391a9ad5b4abfc47c5b19512d3229946e727d94492e1c854d4f792a9db5949d4388e75c70c064394c73893922a8e8132ef96c5043823bb69078ba2cb8940c8b9854603afc1f137c168bf9d4f5984cf97650b302cfe7506c1cca0bc4e29ec5a58b9f920c4b80e6132c6399cc44abe3a629a0f8144b058d4d8381592e3d3c6fad385b9bb92f058a4c730b268aa74931cf4c2b70c54f3181873186d9a11bb47a84db96c02463cace32efc8493f874fe14582c90cb395c38fe6ca65c6f48f2a9299acca06fec000d94cf824c1c517c0f8fd8292197d0cfe0403b020c06247b53131c38342a6b80a6d6052d841ea5f975c181e5e4eabecbcc2d831215d0b268da24936c3f569352731d945f4272570a8a30a1248eea67c278357a3b8015e1032c1ed94f21dc9517e6ade94b0bea01af7d359581d335651772c969f976714965ed50e6b423c0678537b5e17623ee06b6dfb64886f00fe58719b6e0b00af47564f855808b858f9737d884580f7229b0d2116036e44ffa80db104f0efaa75c2524c147d428934c0e755df937219e09dd826e52d077c115b24ac00fc33f65d538811c0a5eaab8d215602be6aba4d0d46019f363ea5445580eb4ddf92540cf0b061fd7488c783bcd78d10ab017f3abed61e620de06f35db842700776befb486580bb856b74c580778a3e52016e249c0bff4afa8aa7ac0fbfac289104f017ed3bc4bd8804e1aafab433c0df8d258a6db46c015639f9c6cc222db1e93b28e336a7b45b7cd80cfda1748ca007ca2bf20af5a50aa7587dc68057cd4b641036d43733a76c9ba769c60fb3d8aedc07ebb5f52a24ec0edbe35c22ec06b5d3728f64d7cdcb949f37d0bf0a0eb2e6137dadefd801ef700fedeb34365f402eef41dd2e33ec07fbb1fd19abd0db8dff38c267806f097fe558aed47dbcf3ca4db77001ff4efd1e60ca0f2c04d6a7f10cb18f887129d057c3c788da4dec5e90f6ed0ed10f67b769f62cf61dec1bb94e83c0e7468833a7a0f17786899f07d6cd0d8d343fc0070b5e53eedd587800fdb57e87618f7b9f580f6ea23fce1f42f35871807dc3bb7da11620267741ea4a201268d92aaaf87173bfe1bcefb1fa42948e824070000'),
(1,0,0,x'1f8b080000000000000303000000000000000000'),
(2,3,1,x'1f8b080000000000000303000000000000000000'),
(3,7,3,x'1f8b080000000000000303000000000000000000'),
(4,7,8,x'1f8b080000000000000303000000000000000000'),
(5,16,20,x'1f8b080000000000000303000000000000000000'),
(6,45,37,x'1f8b080000000000000303000000000000000000');
CREATE UNIQUE INDEX name ON metadata (name);
CREATE UNIQUE INDEX tile_index ON tiles (zoom_level, tile_column, tile_row);
COMMIT;
