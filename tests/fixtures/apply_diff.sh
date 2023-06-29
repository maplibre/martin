#!/usr/bin/env bash

SQL="ATTACH DATABASE @diffDbFilename AS diffDb;
     DELETE FROM tiles WHERE (zoom_level, tile_column, tile_row) IN (SELECT zoom_level, tile_column, tile_row FROM diffDb.tiles WHERE tile_data ISNULL);
     INSERT OR REPLACE INTO tiles (zoom_level, tile_column, tile_row, tile_data) SELECT * FROM diffDb.tiles WHERE tile_data NOTNULL;"

sqlite3 "$1" -cmd ".parameter set @diffDbFilename $2" "$SQL"
