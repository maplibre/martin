CREATE TABLE metadata
  (
     NAME  TEXT,
     value TEXT
  );

CREATE UNIQUE INDEX NAME
  ON metadata (NAME);

CREATE TABLE tiles_data
  (
     tile_data_id INTEGER PRIMARY KEY,
     tile_data    BLOB
  );

CREATE TABLE tiles_shallow (
  zoom_level integer,
  tile_column integer,
  tile_row integer,
  tile_data_id integer,
  primary key(zoom_level,tile_column,tile_row)
) without rowid;

CREATE VIEW tiles
AS
  SELECT tiles_shallow.zoom_level  AS zoom_level,
         tiles_shallow.tile_column AS tile_column,
         tiles_shallow.tile_row    AS tile_row,
         tiles_data.tile_data      AS tile_data
  FROM   tiles_shallow
         JOIN tiles_data
           ON tiles_shallow.tile_data_id = tiles_data.tile_data_id;
