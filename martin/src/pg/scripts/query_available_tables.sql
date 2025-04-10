WITH
--
columns AS (
    -- list of table columns
    SELECT
        ns.nspname AS table_schema,
        class.relname AS table_name,
        attr.attname AS column_name,
        trim(LEADING '_' FROM tp.typname) AS type_name
    FROM pg_attribute AS attr
    INNER JOIN pg_catalog.pg_class AS class ON attr.attrelid = class.oid
    INNER JOIN pg_catalog.pg_namespace AS ns ON class.relnamespace = ns.oid
    INNER JOIN pg_catalog.pg_type AS tp ON attr.atttypid = tp.oid
    WHERE
        NOT attr.attisdropped
        AND attr.attnum > 0
),

--
spatially_indexed_columns AS (
    -- list of columns with spatial indexes
    SELECT
        ns.nspname AS table_schema,
        class.relname AS table_name,
        attr.attname AS column_name
    FROM pg_attribute AS attr
    INNER JOIN pg_class AS class ON attr.attrelid = class.oid
    INNER JOIN pg_namespace AS ns ON class.relnamespace = ns.oid
    INNER JOIN pg_index AS ix
        ON
            class.oid = ix.indrelid
            AND ix.indnkeyatts = 1 -- consider single column indices only
            AND attr.attnum = ix.indkey[0]
    INNER JOIN pg_opclass AS op
        ON
            op.oid = ix.indclass[0]
            AND op.opcname IN (
                'gist_geometry_ops_2d', 'spgist_geometry_ops_2d',
                'brin_geometry_inclusion_ops_2d',
                'gist_geography_ops'
            )
    GROUP BY 1, 2, 3
),

--
annotated_geometry_columns AS (
    -- list of geometry columns with additional metadata
    SELECT
        f_table_schema AS schema,
        f_table_name AS name,
        f_geometry_column AS geom,
        srid,
        type,
        -- 'geometry' AS column_type
        coalesce(class.relkind = 'v', false) AS is_view,
        bool_or(sic.column_name IS NOT null) AS geom_idx
    FROM geometry_columns
    INNER JOIN pg_catalog.pg_class AS class
        ON geometry_columns.f_table_name = class.relname
    INNER JOIN pg_catalog.pg_namespace AS ns
        ON geometry_columns.f_table_schema = ns.nspname
    LEFT JOIN spatially_indexed_columns AS sic
        ON
            geometry_columns.f_table_schema = sic.table_schema
            AND geometry_columns.f_table_name = sic.table_name
            AND geometry_columns.f_geometry_column = sic.column_name
    GROUP BY 1, 2, 3, 4, 5, 6
),

--
annotated_geography_columns AS (
    -- list of geography columns with additional metadata
    SELECT
        f_table_schema AS schema,
        f_table_name AS name,
        f_geography_column AS geom,
        srid,
        type,
        -- 'geography' AS column_type
        coalesce(class.relkind = 'v', false) AS is_view,
        bool_or(sic.column_name IS NOT null) AS geom_idx
    FROM geography_columns
    INNER JOIN pg_catalog.pg_class AS class
        ON geography_columns.f_table_name = class.relname
    INNER JOIN pg_catalog.pg_namespace AS ns
        ON geography_columns.f_table_schema = ns.nspname
    LEFT JOIN spatially_indexed_columns AS sic
        ON
            geography_columns.f_table_schema = sic.table_schema
            AND geography_columns.f_table_name = sic.table_name
            AND geography_columns.f_geography_column = sic.column_name
    GROUP BY 1, 2, 3, 4, 5, 6
),

--
annotated_geo_columns AS (
    SELECT * FROM annotated_geometry_columns
    UNION
    SELECT * FROM annotated_geography_columns
),

--
descriptions AS (
    -- comments on table/views
    SELECT
        pg_namespace.nspname AS schema_name,
        relname AS table_name,
        pg_description.description
    FROM pg_class
    INNER JOIN pg_namespace ON pg_class.relnamespace = pg_namespace.oid
    LEFT JOIN pg_description ON pg_class.oid = pg_description.objoid
    WHERE relkind = 'r' OR relkind = 'v'
)

SELECT
    schema,
    name,
    geom,
    srid,
    type,
    is_view,
    geom_idx,
    dc.description,
    coalesce(
        jsonb_object_agg(columns.column_name, columns.type_name)
        FILTER (
            WHERE columns.column_name IS NOT null
            AND columns.type_name != 'geometry'
            AND columns.type_name != 'geography'
        ),
        '{}'::jsonb
    ) AS properties
FROM annotated_geo_columns AS gc
LEFT JOIN columns
    ON
        gc.schema = columns.table_schema
        AND gc.name = columns.table_name
        AND gc.geom != columns.column_name
LEFT JOIN descriptions AS dc
    ON
        gc.schema = dc.schema_name
        AND gc.name = dc.table_name
GROUP BY
    gc.schema, gc.name, gc.geom, gc.srid, gc.type, gc.is_view, gc.geom_idx, dc.description;
