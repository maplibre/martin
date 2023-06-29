#!/usr/bin/env bash

echo "IN HERE"
SQL="ATTACH DATABASE @newDbFilename AS newDb;
     DELETE FROM tiles WHERE (zoom_level, tile_column, tile_row) IN (SELECT zoom_level, tile_column, tile_row FROM newDb.tiles WHERE tile_data ISNULL);
     INSERT OR REPLACE INTO tiles (zoom_level, tile_column, tile_row, tile_data) SELECT * FROM newDb.tiles WHERE tile_data NOTNULL;"

sqlite3 "$1" -cmd ".parameter set @newDbFilename $2" "$SQL"
