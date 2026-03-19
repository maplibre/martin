CREATE TABLE metadata (name text, value text);
CREATE UNIQUE INDEX name ON metadata (name);
CREATE TABLE tiles_shallow (
  zoom_level integer,
  tile_column integer,
  tile_row integer,
  tile_data_id integer,
  PRIMARY KEY(zoom_level, tile_column, tile_row)
) WITHOUT ROWID;
CREATE TABLE tiles_data (
  tile_data_id integer PRIMARY KEY,
  tile_data blob
);
CREATE VIEW tiles AS
  SELECT tiles_shallow.zoom_level, tiles_shallow.tile_column, tiles_shallow.tile_row, tiles_data.tile_data
  FROM tiles_shallow
  JOIN tiles_data ON tiles_shallow.tile_data_id = tiles_data.tile_data_id;
