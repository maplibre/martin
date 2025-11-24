PRAGMA foreign_keys=OFF;
BEGIN TRANSACTION;
CREATE TABLE map (
   zoom_level INTEGER,
   tile_column INTEGER,
   tile_row INTEGER,
   tile_id TEXT,
   grid_id TEXT
);
INSERT INTO map VALUES
(2,2,2,'035e1077aab736ad34208aaea571d6ac','a592787e1b98714c9af7ba3e494166db'),
(1,0,0,'f7cb51a3403b156551bfa77023c81f8a','f4e6039f6261ecdf5a9ca153121e5ad7'),
(1,0,1,'58e516125a2c9009f094ca995b06425c','38119e84848bfb161d4d81e07d241b58'),
(1,1,0,'d8018fba714e93c29500adb778b587a5','57a5641e4893608878e715fd628870cd'),
(1,1,1,'d8018fba714e93c29500adb778b587a5','710f5a40afdc3155cf458ebcfdd76c09');
CREATE TABLE grid_key (
    grid_id TEXT,
    key_name TEXT
);
INSERT INTO grid_key VALUES
('a592787e1b98714c9af7ba3e494166db','3'),
('a592787e1b98714c9af7ba3e494166db','4'),
('a592787e1b98714c9af7ba3e494166db','10'),
('a592787e1b98714c9af7ba3e494166db','13'),
('a592787e1b98714c9af7ba3e494166db','15'),
('a592787e1b98714c9af7ba3e494166db','17'),
('a592787e1b98714c9af7ba3e494166db','19'),
('a592787e1b98714c9af7ba3e494166db','24'),
('a592787e1b98714c9af7ba3e494166db','27'),
('a592787e1b98714c9af7ba3e494166db','33'),
('a592787e1b98714c9af7ba3e494166db','34'),
('a592787e1b98714c9af7ba3e494166db','38'),
('a592787e1b98714c9af7ba3e494166db','39'),
('a592787e1b98714c9af7ba3e494166db','40'),
('a592787e1b98714c9af7ba3e494166db','42'),
('a592787e1b98714c9af7ba3e494166db','43'),
('a592787e1b98714c9af7ba3e494166db','44'),
('a592787e1b98714c9af7ba3e494166db','45'),
('a592787e1b98714c9af7ba3e494166db','46'),
('a592787e1b98714c9af7ba3e494166db','47'),
('a592787e1b98714c9af7ba3e494166db','49'),
('a592787e1b98714c9af7ba3e494166db','53'),
('a592787e1b98714c9af7ba3e494166db','59'),
('a592787e1b98714c9af7ba3e494166db','62'),
('a592787e1b98714c9af7ba3e494166db','64'),
('a592787e1b98714c9af7ba3e494166db','65'),
('a592787e1b98714c9af7ba3e494166db','66'),
('a592787e1b98714c9af7ba3e494166db','67'),
('a592787e1b98714c9af7ba3e494166db','68'),
('a592787e1b98714c9af7ba3e494166db','69'),
('a592787e1b98714c9af7ba3e494166db','70'),
('a592787e1b98714c9af7ba3e494166db','71'),
('a592787e1b98714c9af7ba3e494166db','72'),
('a592787e1b98714c9af7ba3e494166db','74'),
('a592787e1b98714c9af7ba3e494166db','77'),
('a592787e1b98714c9af7ba3e494166db','79'),
('a592787e1b98714c9af7ba3e494166db','85'),
('a592787e1b98714c9af7ba3e494166db','87'),
('a592787e1b98714c9af7ba3e494166db','89'),
('a592787e1b98714c9af7ba3e494166db','94'),
('a592787e1b98714c9af7ba3e494166db','95'),
('a592787e1b98714c9af7ba3e494166db','96'),
('a592787e1b98714c9af7ba3e494166db','99'),
('a592787e1b98714c9af7ba3e494166db','101'),
('a592787e1b98714c9af7ba3e494166db','102'),
('a592787e1b98714c9af7ba3e494166db','104'),
('a592787e1b98714c9af7ba3e494166db','105'),
('a592787e1b98714c9af7ba3e494166db','106'),
('a592787e1b98714c9af7ba3e494166db','107'),
('a592787e1b98714c9af7ba3e494166db','109'),
('a592787e1b98714c9af7ba3e494166db','113'),
('a592787e1b98714c9af7ba3e494166db','115'),
('a592787e1b98714c9af7ba3e494166db','116'),
('a592787e1b98714c9af7ba3e494166db','119'),
('a592787e1b98714c9af7ba3e494166db','121'),
('a592787e1b98714c9af7ba3e494166db','127'),
('a592787e1b98714c9af7ba3e494166db','130'),
('a592787e1b98714c9af7ba3e494166db','137'),
('a592787e1b98714c9af7ba3e494166db','140'),
('a592787e1b98714c9af7ba3e494166db','141'),
('a592787e1b98714c9af7ba3e494166db','142'),
('a592787e1b98714c9af7ba3e494166db','145'),
('a592787e1b98714c9af7ba3e494166db','147'),
('a592787e1b98714c9af7ba3e494166db','149'),
('a592787e1b98714c9af7ba3e494166db','151'),
('a592787e1b98714c9af7ba3e494166db','152'),
('a592787e1b98714c9af7ba3e494166db','155'),
('a592787e1b98714c9af7ba3e494166db','156'),
('a592787e1b98714c9af7ba3e494166db','157'),
('a592787e1b98714c9af7ba3e494166db','159'),
('a592787e1b98714c9af7ba3e494166db','161'),
('a592787e1b98714c9af7ba3e494166db','162'),
('a592787e1b98714c9af7ba3e494166db','164'),
('a592787e1b98714c9af7ba3e494166db','165'),
('a592787e1b98714c9af7ba3e494166db','166'),
('a592787e1b98714c9af7ba3e494166db','168'),
('a592787e1b98714c9af7ba3e494166db','169'),
('a592787e1b98714c9af7ba3e494166db','170'),
('a592787e1b98714c9af7ba3e494166db','173'),
('a592787e1b98714c9af7ba3e494166db','174'),
('a592787e1b98714c9af7ba3e494166db','176'),
('a592787e1b98714c9af7ba3e494166db','177'),
('a592787e1b98714c9af7ba3e494166db','179'),
('a592787e1b98714c9af7ba3e494166db','181'),
('a592787e1b98714c9af7ba3e494166db','182'),
('a592787e1b98714c9af7ba3e494166db','183'),
('a592787e1b98714c9af7ba3e494166db','184'),
('a592787e1b98714c9af7ba3e494166db','185'),
('a592787e1b98714c9af7ba3e494166db','187'),
('a592787e1b98714c9af7ba3e494166db','188'),
('a592787e1b98714c9af7ba3e494166db','189'),
('a592787e1b98714c9af7ba3e494166db','190'),
('a592787e1b98714c9af7ba3e494166db','191'),
('a592787e1b98714c9af7ba3e494166db','196'),
('a592787e1b98714c9af7ba3e494166db','199'),
('a592787e1b98714c9af7ba3e494166db','200'),
('a592787e1b98714c9af7ba3e494166db','202'),
('a592787e1b98714c9af7ba3e494166db','204'),
('a592787e1b98714c9af7ba3e494166db','205'),
('a592787e1b98714c9af7ba3e494166db','207'),
('a592787e1b98714c9af7ba3e494166db','211'),
('a592787e1b98714c9af7ba3e494166db','212'),
('a592787e1b98714c9af7ba3e494166db','213'),
('a592787e1b98714c9af7ba3e494166db','214'),
('a592787e1b98714c9af7ba3e494166db','215'),
('a592787e1b98714c9af7ba3e494166db','217'),
('a592787e1b98714c9af7ba3e494166db','222'),
('a592787e1b98714c9af7ba3e494166db','224'),
('a592787e1b98714c9af7ba3e494166db','225'),
('a592787e1b98714c9af7ba3e494166db','226'),
('a592787e1b98714c9af7ba3e494166db','227'),
('a592787e1b98714c9af7ba3e494166db','228'),
('a592787e1b98714c9af7ba3e494166db','229'),
('a592787e1b98714c9af7ba3e494166db','232'),
('a592787e1b98714c9af7ba3e494166db','235'),
('a592787e1b98714c9af7ba3e494166db','237'),
('a592787e1b98714c9af7ba3e494166db','240'),
('a592787e1b98714c9af7ba3e494166db','241'),
('a592787e1b98714c9af7ba3e494166db','242'),
('a592787e1b98714c9af7ba3e494166db','243'),
('38119e84848bfb161d4d81e07d241b58','23'),
('38119e84848bfb161d4d81e07d241b58','27'),
('38119e84848bfb161d4d81e07d241b58','34'),
('38119e84848bfb161d4d81e07d241b58','40'),
('38119e84848bfb161d4d81e07d241b58','44'),
('38119e84848bfb161d4d81e07d241b58','49'),
('38119e84848bfb161d4d81e07d241b58','53'),
('38119e84848bfb161d4d81e07d241b58','55'),
('38119e84848bfb161d4d81e07d241b58','63'),
('38119e84848bfb161d4d81e07d241b58','64'),
('38119e84848bfb161d4d81e07d241b58','68'),
('38119e84848bfb161d4d81e07d241b58','74'),
('38119e84848bfb161d4d81e07d241b58','79'),
('38119e84848bfb161d4d81e07d241b58','82'),
('38119e84848bfb161d4d81e07d241b58','83'),
('38119e84848bfb161d4d81e07d241b58','85'),
('38119e84848bfb161d4d81e07d241b58','89'),
('38119e84848bfb161d4d81e07d241b58','90'),
('38119e84848bfb161d4d81e07d241b58','92'),
('38119e84848bfb161d4d81e07d241b58','95'),
('38119e84848bfb161d4d81e07d241b58','97'),
('38119e84848bfb161d4d81e07d241b58','104'),
('38119e84848bfb161d4d81e07d241b58','107'),
('38119e84848bfb161d4d81e07d241b58','126'),
('38119e84848bfb161d4d81e07d241b58','137'),
('38119e84848bfb161d4d81e07d241b58','142'),
('38119e84848bfb161d4d81e07d241b58','145'),
('38119e84848bfb161d4d81e07d241b58','152'),
('38119e84848bfb161d4d81e07d241b58','162'),
('38119e84848bfb161d4d81e07d241b58','171'),
('38119e84848bfb161d4d81e07d241b58','180'),
('38119e84848bfb161d4d81e07d241b58','185'),
('38119e84848bfb161d4d81e07d241b58','187'),
('38119e84848bfb161d4d81e07d241b58','191'),
('38119e84848bfb161d4d81e07d241b58','196'),
('38119e84848bfb161d4d81e07d241b58','197'),
('38119e84848bfb161d4d81e07d241b58','201'),
('38119e84848bfb161d4d81e07d241b58','204'),
('38119e84848bfb161d4d81e07d241b58','220'),
('38119e84848bfb161d4d81e07d241b58','228'),
('38119e84848bfb161d4d81e07d241b58','231'),
('38119e84848bfb161d4d81e07d241b58','232'),
('38119e84848bfb161d4d81e07d241b58','233'),
('f4e6039f6261ecdf5a9ca153121e5ad7','10'),
('f4e6039f6261ecdf5a9ca153121e5ad7','13'),
('f4e6039f6261ecdf5a9ca153121e5ad7','33'),
('f4e6039f6261ecdf5a9ca153121e5ad7','34'),
('f4e6039f6261ecdf5a9ca153121e5ad7','42'),
('f4e6039f6261ecdf5a9ca153121e5ad7','49'),
('f4e6039f6261ecdf5a9ca153121e5ad7','65'),
('f4e6039f6261ecdf5a9ca153121e5ad7','72'),
('f4e6039f6261ecdf5a9ca153121e5ad7','119'),
('f4e6039f6261ecdf5a9ca153121e5ad7','173'),
('f4e6039f6261ecdf5a9ca153121e5ad7','181'),
('f4e6039f6261ecdf5a9ca153121e5ad7','182'),
('f4e6039f6261ecdf5a9ca153121e5ad7','193'),
('f4e6039f6261ecdf5a9ca153121e5ad7','227'),
('f4e6039f6261ecdf5a9ca153121e5ad7','239'),
('57a5641e4893608878e715fd628870cd','4'),
('57a5641e4893608878e715fd628870cd','13'),
('57a5641e4893608878e715fd628870cd','15'),
('57a5641e4893608878e715fd628870cd','17'),
('57a5641e4893608878e715fd628870cd','38'),
('57a5641e4893608878e715fd628870cd','46'),
('57a5641e4893608878e715fd628870cd','47'),
('57a5641e4893608878e715fd628870cd','72'),
('57a5641e4893608878e715fd628870cd','77'),
('57a5641e4893608878e715fd628870cd','99'),
('57a5641e4893608878e715fd628870cd','116'),
('57a5641e4893608878e715fd628870cd','119'),
('57a5641e4893608878e715fd628870cd','140'),
('57a5641e4893608878e715fd628870cd','151'),
('57a5641e4893608878e715fd628870cd','155'),
('57a5641e4893608878e715fd628870cd','157'),
('57a5641e4893608878e715fd628870cd','158'),
('57a5641e4893608878e715fd628870cd','168'),
('57a5641e4893608878e715fd628870cd','176'),
('57a5641e4893608878e715fd628870cd','195'),
('57a5641e4893608878e715fd628870cd','200'),
('57a5641e4893608878e715fd628870cd','218'),
('57a5641e4893608878e715fd628870cd','224'),
('57a5641e4893608878e715fd628870cd','225'),
('57a5641e4893608878e715fd628870cd','236'),
('57a5641e4893608878e715fd628870cd','241'),
('57a5641e4893608878e715fd628870cd','242'),
('57a5641e4893608878e715fd628870cd','243'),
('710f5a40afdc3155cf458ebcfdd76c09','3'),
('710f5a40afdc3155cf458ebcfdd76c09','6'),
('710f5a40afdc3155cf458ebcfdd76c09','7'),
('710f5a40afdc3155cf458ebcfdd76c09','9'),
('710f5a40afdc3155cf458ebcfdd76c09','18'),
('710f5a40afdc3155cf458ebcfdd76c09','19'),
('710f5a40afdc3155cf458ebcfdd76c09','22'),
('710f5a40afdc3155cf458ebcfdd76c09','24'),
('710f5a40afdc3155cf458ebcfdd76c09','25'),
('710f5a40afdc3155cf458ebcfdd76c09','28'),
('710f5a40afdc3155cf458ebcfdd76c09','30'),
('710f5a40afdc3155cf458ebcfdd76c09','39'),
('710f5a40afdc3155cf458ebcfdd76c09','41'),
('710f5a40afdc3155cf458ebcfdd76c09','43'),
('710f5a40afdc3155cf458ebcfdd76c09','45'),
('710f5a40afdc3155cf458ebcfdd76c09','46'),
('710f5a40afdc3155cf458ebcfdd76c09','47'),
('710f5a40afdc3155cf458ebcfdd76c09','58'),
('710f5a40afdc3155cf458ebcfdd76c09','59'),
('710f5a40afdc3155cf458ebcfdd76c09','62'),
('710f5a40afdc3155cf458ebcfdd76c09','64'),
('710f5a40afdc3155cf458ebcfdd76c09','66'),
('710f5a40afdc3155cf458ebcfdd76c09','67'),
('710f5a40afdc3155cf458ebcfdd76c09','68'),
('710f5a40afdc3155cf458ebcfdd76c09','69'),
('710f5a40afdc3155cf458ebcfdd76c09','70'),
('710f5a40afdc3155cf458ebcfdd76c09','71'),
('710f5a40afdc3155cf458ebcfdd76c09','74'),
('710f5a40afdc3155cf458ebcfdd76c09','79'),
('710f5a40afdc3155cf458ebcfdd76c09','80'),
('710f5a40afdc3155cf458ebcfdd76c09','86'),
('710f5a40afdc3155cf458ebcfdd76c09','87'),
('710f5a40afdc3155cf458ebcfdd76c09','96'),
('710f5a40afdc3155cf458ebcfdd76c09','98'),
('710f5a40afdc3155cf458ebcfdd76c09','99'),
('710f5a40afdc3155cf458ebcfdd76c09','101'),
('710f5a40afdc3155cf458ebcfdd76c09','105'),
('710f5a40afdc3155cf458ebcfdd76c09','106'),
('710f5a40afdc3155cf458ebcfdd76c09','108'),
('710f5a40afdc3155cf458ebcfdd76c09','109'),
('710f5a40afdc3155cf458ebcfdd76c09','113'),
('710f5a40afdc3155cf458ebcfdd76c09','115'),
('710f5a40afdc3155cf458ebcfdd76c09','116'),
('710f5a40afdc3155cf458ebcfdd76c09','117'),
('710f5a40afdc3155cf458ebcfdd76c09','118'),
('710f5a40afdc3155cf458ebcfdd76c09','121'),
('710f5a40afdc3155cf458ebcfdd76c09','123'),
('710f5a40afdc3155cf458ebcfdd76c09','124'),
('710f5a40afdc3155cf458ebcfdd76c09','127'),
('710f5a40afdc3155cf458ebcfdd76c09','130'),
('710f5a40afdc3155cf458ebcfdd76c09','132'),
('710f5a40afdc3155cf458ebcfdd76c09','134'),
('710f5a40afdc3155cf458ebcfdd76c09','145'),
('710f5a40afdc3155cf458ebcfdd76c09','146'),
('710f5a40afdc3155cf458ebcfdd76c09','147'),
('710f5a40afdc3155cf458ebcfdd76c09','149'),
('710f5a40afdc3155cf458ebcfdd76c09','156'),
('710f5a40afdc3155cf458ebcfdd76c09','159'),
('710f5a40afdc3155cf458ebcfdd76c09','161'),
('710f5a40afdc3155cf458ebcfdd76c09','164'),
('710f5a40afdc3155cf458ebcfdd76c09','165'),
('710f5a40afdc3155cf458ebcfdd76c09','166'),
('710f5a40afdc3155cf458ebcfdd76c09','169'),
('710f5a40afdc3155cf458ebcfdd76c09','170'),
('710f5a40afdc3155cf458ebcfdd76c09','174'),
('710f5a40afdc3155cf458ebcfdd76c09','177'),
('710f5a40afdc3155cf458ebcfdd76c09','179'),
('710f5a40afdc3155cf458ebcfdd76c09','184'),
('710f5a40afdc3155cf458ebcfdd76c09','185'),
('710f5a40afdc3155cf458ebcfdd76c09','188'),
('710f5a40afdc3155cf458ebcfdd76c09','189'),
('710f5a40afdc3155cf458ebcfdd76c09','190'),
('710f5a40afdc3155cf458ebcfdd76c09','199'),
('710f5a40afdc3155cf458ebcfdd76c09','200'),
('710f5a40afdc3155cf458ebcfdd76c09','202'),
('710f5a40afdc3155cf458ebcfdd76c09','205'),
('710f5a40afdc3155cf458ebcfdd76c09','207'),
('710f5a40afdc3155cf458ebcfdd76c09','211'),
('710f5a40afdc3155cf458ebcfdd76c09','213'),
('710f5a40afdc3155cf458ebcfdd76c09','214'),
('710f5a40afdc3155cf458ebcfdd76c09','215'),
('710f5a40afdc3155cf458ebcfdd76c09','216'),
('710f5a40afdc3155cf458ebcfdd76c09','217'),
('710f5a40afdc3155cf458ebcfdd76c09','221'),
('710f5a40afdc3155cf458ebcfdd76c09','222'),
('710f5a40afdc3155cf458ebcfdd76c09','223'),
('710f5a40afdc3155cf458ebcfdd76c09','225'),
('710f5a40afdc3155cf458ebcfdd76c09','226'),
('710f5a40afdc3155cf458ebcfdd76c09','228'),
('710f5a40afdc3155cf458ebcfdd76c09','229'),
('710f5a40afdc3155cf458ebcfdd76c09','235'),
('710f5a40afdc3155cf458ebcfdd76c09','240');
CREATE TABLE keymap (
    key_name TEXT,
    key_json TEXT
);
INSERT INTO keymap VALUES('3','{"admin":"Afghanistan","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('4','{"admin":"Angola","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('6','{"admin":"Albania","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('7','{"admin":"Aland","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('9','{"admin":"United Arab Emirates","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('10','{"admin":"Argentina","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('13','{"admin":"Antarctica","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('15','{"admin":"French Southern and Antarctic Lands","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('17','{"admin":"Australia","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('18','{"admin":"Austria","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('19','{"admin":"Azerbaijan","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('22','{"admin":"Benin","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('23','{"admin":"Burkina Faso","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('24','{"admin":"Bangladesh","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('25','{"admin":"Bulgaria","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('27','{"admin":"The Bahamas","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('28','{"admin":"Bosnia and Herzegovina","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('30','{"admin":"Belarus","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('33','{"admin":"Bolivia","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('34','{"admin":"Brazil","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('38','{"admin":"Botswana","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('39','{"admin":"Central African Republic","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('40','{"admin":"Canada","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('41','{"admin":"Switzerland","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('42','{"admin":"Chile","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('43','{"admin":"China","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('44','{"admin":"Ivory Coast","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('45','{"admin":"Cameroon","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('46','{"admin":"Democratic Republic of the Congo","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('47','{"admin":"Republic of the Congo","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('49','{"admin":"Colombia","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('53','{"admin":"Cuba","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('55','{"admin":"Cayman Islands","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('58','{"admin":"Czech Republic","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('59','{"admin":"Germany","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('62','{"admin":"Denmark","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('63','{"admin":"Dominican Republic","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('64','{"admin":"Algeria","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('65','{"admin":"Ecuador","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('66','{"admin":"Egypt","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('67','{"admin":"Eritrea","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('68','{"admin":"Spain","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('69','{"admin":"Estonia","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('70','{"admin":"Ethiopia","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('71','{"admin":"Finland","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('72','{"admin":"Fiji","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('74','{"admin":"France","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('77','{"admin":"Gabon","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('79','{"admin":"United Kingdom","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('80','{"admin":"Georgia","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('82','{"admin":"Ghana","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('83','{"admin":"Guinea","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('85','{"admin":"Guinea Bissau","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('86','{"admin":"Equatorial Guinea","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('87','{"admin":"Greece","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('89','{"admin":"Greenland","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('90','{"admin":"Guatemala","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('92','{"admin":"Guyana","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('94','{"admin":"Heard Island and McDonald Islands","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('95','{"admin":"Honduras","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('96','{"admin":"Croatia","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('97','{"admin":"Haiti","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('98','{"admin":"Hungary","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('99','{"admin":"Indonesia","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('101','{"admin":"India","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('102','{"admin":"Indian Ocean Territories","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('104','{"admin":"Ireland","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('105','{"admin":"Iran","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('106','{"admin":"Iraq","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('107','{"admin":"Iceland","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('108','{"admin":"Israel","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('109','{"admin":"Italy","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('113','{"admin":"Japan","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('115','{"admin":"Kazakhstan","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('116','{"admin":"Kenya","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('117','{"admin":"Kyrgyzstan","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('118','{"admin":"Cambodia","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('119','{"admin":"Kiribati","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('121','{"admin":"South Korea","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('123','{"admin":"Kuwait","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('124','{"admin":"Laos","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('126','{"admin":"Liberia","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('127','{"admin":"Libya","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('130','{"admin":"Sri Lanka","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('132','{"admin":"Lithuania","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('134','{"admin":"Latvia","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('137','{"admin":"Morocco","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('140','{"admin":"Madagascar","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('141','{"admin":"Maldives","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('142','{"admin":"Mexico","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('145','{"admin":"Mali","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('146','{"admin":"Malta","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('147','{"admin":"Myanmar","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('149','{"admin":"Mongolia","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('151','{"admin":"Mozambique","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('152','{"admin":"Mauritania","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('155','{"admin":"Malawi","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('156','{"admin":"Malaysia","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('157','{"admin":"Namibia","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('158','{"admin":"New Caledonia","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('159','{"admin":"Niger","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('161','{"admin":"Nigeria","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('162','{"admin":"Nicaragua","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('164','{"admin":"Netherlands","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('165','{"admin":"Norway","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('166','{"admin":"Nepal","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('168','{"admin":"New Zealand","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('169','{"admin":"Oman","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('170','{"admin":"Pakistan","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('171','{"admin":"Panama","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('173','{"admin":"Peru","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('174','{"admin":"Philippines","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('176','{"admin":"Papua New Guinea","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('177','{"admin":"Poland","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('179','{"admin":"North Korea","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('180','{"admin":"Portugal","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('181','{"admin":"Paraguay","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('182','{"admin":"French Polynesia","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('183','{"admin":"Qatar","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('184','{"admin":"Romania","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('185','{"admin":"Russia","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('187','{"admin":"Western Sahara","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('188','{"admin":"Saudi Arabia","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('189','{"admin":"Sudan","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('190','{"admin":"South Sudan","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('191','{"admin":"Senegal","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('193','{"admin":"South Georgia and South Sandwich Islands","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('195','{"admin":"Solomon Islands","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('196','{"admin":"Sierra Leone","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('197','{"admin":"El Salvador","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('199','{"admin":"Somaliland","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('200','{"admin":"Somalia","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('201','{"admin":"Saint Pierre and Miquelon","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('202','{"admin":"Republic of Serbia","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('204','{"admin":"Suriname","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('205','{"admin":"Slovakia","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('207','{"admin":"Sweden","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('211','{"admin":"Syria","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('212','{"admin":"Turks and Caicos Islands","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('213','{"admin":"Chad","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('214','{"admin":"Togo","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('215','{"admin":"Thailand","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('216','{"admin":"Tajikistan","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('217','{"admin":"Turkmenistan","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('218','{"admin":"East Timor","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('220','{"admin":"Trinidad and Tobago","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('221','{"admin":"Tunisia","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('222','{"admin":"Turkey","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('223','{"admin":"Taiwan","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('224','{"admin":"United Republic of Tanzania","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('225','{"admin":"Uganda","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('226','{"admin":"Ukraine","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('227','{"admin":"Uruguay","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('228','{"admin":"United States of America","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('229','{"admin":"Uzbekistan","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('231','{"admin":"Saint Vincent and the Grenadines","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('232','{"admin":"Venezuela","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('233','{"admin":"British Virgin Islands","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('235','{"admin":"Vietnam","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('236','{"admin":"Vanuatu","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('237','{"admin":"West Bank","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('239','{"admin":"Samoa","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('240','{"admin":"Yemen","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('241','{"admin":"South Africa","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('242','{"admin":"Zambia","flag_png":"iVBORw0KGgo"}');
INSERT INTO keymap VALUES('243','{"admin":"Zimbabwe","flag_png":"iVBORw0KGgo"}');
CREATE TABLE grid_utfgrid (
    grid_id TEXT,
    grid_utfgrid BLOB
);
INSERT INTO grid_utfgrid VALUES('a592787e1b98714c9af7ba3e494166db',X'789C03');
INSERT INTO grid_utfgrid VALUES('38119e84848bfb161d4d81e07d241b58',X'789C03');
INSERT INTO grid_utfgrid VALUES('f4e6039f6261ecdf5a9ca153121e5ad7',X'789C03');
INSERT INTO grid_utfgrid VALUES('57a5641e4893608878e715fd628870cd',X'789C03');
INSERT INTO grid_utfgrid VALUES('710f5a40afdc3155cf458ebcfdd76c09',X'789C03');
CREATE TABLE images (
    tile_data blob,
    tile_id text
);
INSERT INTO images VALUES(X'FFD80000FFD9','035e1077aab736ad34208aaea571d6ac');
INSERT INTO images VALUES(X'FFD8FFD9','f7cb51a3403b156551bfa77023c81f8a');
INSERT INTO images VALUES(X'FFD8FFD9','58e516125a2c9009f094ca995b06425c');
INSERT INTO images VALUES(X'FFD80000FFD9','d8018fba714e93c29500adb778b587a5');
CREATE TABLE metadata (
    name text,
    value text
);
INSERT INTO metadata VALUES('bounds','-180,-85.0511,180,85.0511');
INSERT INTO metadata VALUES('minzoom','0');
INSERT INTO metadata VALUES('maxzoom','1');
INSERT INTO metadata VALUES('legend','<div style="text-align:center;">' || x'0A0A' || '<div style="font:12pt/16pt Georgia,serif;">Geography Class</div>' || x'0A' || '<div style="font:italic 10pt/16pt Georgia,serif;">by MapBox</div>' || x'0A0A' || '<img src="data:image/png;base64,iVBORw0KGgo">' || x'0A' || '</div>');
INSERT INTO metadata VALUES('name','Geography Class');
INSERT INTO metadata VALUES('description','A modified version of one of the example maps that comes with TileMill - a bright & colorful world map that blends retro and high-tech with its folded paper texture and interactive flag tooltips.');
INSERT INTO metadata VALUES('attribution','');
INSERT INTO metadata VALUES('template','{{#__location__}}{{/__location__}}{{#__teaser__}}<div style="text-align:center;">' || x'0A0A' || '<img src="data:image/png;base64,{{flag_png}}" style="-moz-box-shadow:0px 1px 3px #222;-webkit-box-shadow:0px 1px 5px #222;box-shadow:0px 1px 3px #222;"><br>' || x'0A' || '<strong>{{admin}}</strong>' || x'0A0A' || '</div>{{/__teaser__}}{{#__full__}}{{/__full__}}');
INSERT INTO metadata VALUES('version','1.0.0');
CREATE VIEW tiles AS
    SELECT
        map.zoom_level AS zoom_level,
        map.tile_column AS tile_column,
        map.tile_row AS tile_row,
        images.tile_data AS tile_data
    FROM map
    JOIN images ON images.tile_id = map.tile_id;
CREATE VIEW grids AS
    SELECT
        map.zoom_level AS zoom_level,
        map.tile_column AS tile_column,
        map.tile_row AS tile_row,
        grid_utfgrid.grid_utfgrid AS grid
    FROM map
    JOIN grid_utfgrid ON grid_utfgrid.grid_id = map.grid_id;
CREATE VIEW grid_data AS
    SELECT
        map.zoom_level AS zoom_level,
        map.tile_column AS tile_column,
        map.tile_row AS tile_row,
        keymap.key_name AS key_name,
        keymap.key_json AS key_json
    FROM map
    JOIN grid_key ON map.grid_id = grid_key.grid_id
    JOIN keymap ON grid_key.key_name = keymap.key_name;
CREATE UNIQUE INDEX map_index ON map (zoom_level, tile_column, tile_row);
CREATE UNIQUE INDEX grid_key_lookup ON grid_key (grid_id, key_name);
CREATE UNIQUE INDEX keymap_lookup ON keymap (key_name);
CREATE UNIQUE INDEX grid_utfgrid_lookup ON grid_utfgrid (grid_id);
CREATE UNIQUE INDEX images_id ON images (tile_id);
CREATE UNIQUE INDEX name ON metadata (name);
COMMIT;
