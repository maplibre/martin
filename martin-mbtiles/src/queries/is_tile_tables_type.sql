SELECT  (
            -- Has a "tiles" table
            SELECT COUNT(*) = 1
            FROM sqlite_master
            WHERE name='tiles' AND type='table'
            --
        ) AND (
            -- "tiles" table's columns and their types are as expected:
            -- 4 non-null columns (zoom_level, tile_column, tile_row, tile_data).
            -- The order is not important
            SELECT COUNT(*) = 4
            FROM pragma_table_info('tiles')
            WHERE "notnull"=0
                AND ((name="zoom_level" AND type="INTEGER") OR
                    (name="tile_column" AND type="INTEGER") OR
                     (name="tile_row" AND type="INTEGER") OR
                     (name="tile_data" AND type="BLOB"))
            --
        ) as is_valid;
