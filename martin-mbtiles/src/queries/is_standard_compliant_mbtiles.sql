SELECT 1 FROM
        (SELECT 1
        FROM sqlite_master
        WHERE name='tiles' AND type='table')
        JOIN
        (SELECT 1
        FROM (SELECT COUNT(*) AS col_count
              FROM pragma_table_info('tiles')
              WHERE "notnull"=0 AND ((name="zoom_level" AND type="INTEGER") OR
                                      (name="tile_column" AND type="INTEGER") OR
                                      (name="tile_row" AND type="INTEGER") OR
                                      (name="tile_data" AND type="BLOB")))
        WHERE col_count = 4)