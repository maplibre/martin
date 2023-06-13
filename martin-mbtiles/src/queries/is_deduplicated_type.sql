SELECT (
           -- Has a "map" table
           SELECT COUNT(*) = 1
           FROM sqlite_master
           WHERE name = 'map'
             AND type = 'table'
           --
       ) AND (
           -- "map" table's columns and their types are as expected:
           -- 4 non-null columns (zoom_level, tile_column, tile_row, tile_id).
           -- The order is not important
           SELECT COUNT(*) = 4
           FROM pragma_table_info('map')
           WHERE "notnull" = 0
             AND ((name = "zoom_level" AND type = "INTEGER") OR
                  (name = "tile_column" AND type = "INTEGER") OR
                  (name = "tile_row" AND type = "INTEGER") OR
                  (name = "tile_id" AND type = "TEXT"))
           --
       ) AND (
           -- Has a "images" table
           SELECT COUNT(*) = 1
           FROM sqlite_master
           WHERE name = 'images'
             AND type = 'table'
           --
       ) AND (
           -- "images" table's columns and their types are as expected:
           -- 2 non-null columns (tile_id, tile_data).
           -- The order is not important
           SELECT COUNT(*) = 2
           FROM pragma_table_info('images')
           WHERE "notnull" = 0
             AND ((name = "tile_id" AND type = "TEXT") OR
                  (name = "tile_data" AND type = "BLOB"))
           --
       ) AS is_valid;
