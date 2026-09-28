WITH RECURSIVE
--
domains AS (
    -- each domain with every type below it, down to the first one that is not a domain
    SELECT
        dom.oid AS domain_oid,
        base.oid AS base_oid,
        base.typname AS base_name,
        base.typtype AS base_typtype
    FROM pg_catalog.pg_type AS dom
    INNER JOIN pg_catalog.pg_type AS base ON dom.typbasetype = base.oid
    WHERE dom.typtype = 'd'
    UNION ALL
    SELECT
        domains.domain_oid,
        base.oid AS base_oid,
        base.typname AS base_name,
        base.typtype AS base_typtype
    FROM domains
    INNER JOIN pg_catalog.pg_type AS dom ON domains.base_oid = dom.oid
    INNER JOIN pg_catalog.pg_type AS base ON dom.typbasetype = base.oid
    WHERE domains.base_typtype = 'd'
),

--
geo_columns AS (
    -- every geometry and geography column the user can read in the schemas $1 lists lowercased (all when NULL), as the geometry_columns and geography_columns views list them
    SELECT
        cls.oid AS relid,
        ns.nspname AS schema, -- noqa: RF04
        cls.relname AS name, -- noqa: RF04
        attr.attname AS geom,
        attr.attnum,
        cls.relkind,
        CASE
            WHEN tp.typname = 'geography' THEN postgis_typmod_srid(attr.atttypmod)
            ELSE coalesce(
                nullif(postgis_typmod_srid(attr.atttypmod), 0),
                (
                    SELECT (regexp_match(pg_get_constraintdef(con.oid), 'srid\(\w+\)\s*=\s*(\d+)', 'i'))[1]::integer
                    FROM pg_catalog.pg_constraint AS con
                    WHERE
                        con.conrelid = cls.oid
                        AND attr.attnum = any(con.conkey)
                        AND pg_get_constraintdef(con.oid) ~* 'srid\(\w+\)\s*=\s*\d+'
                    ORDER BY con.oid
                    LIMIT 1
                ),
                0
            )
        END AS srid,
        CASE
            WHEN tp.typname = 'geography' THEN postgis_typmod_type(attr.atttypmod)
            ELSE replace(replace(coalesce(
                nullif(upper(postgis_typmod_type(attr.atttypmod)), 'GEOMETRY'),
                (
                    SELECT (regexp_match(pg_get_constraintdef(con.oid), 'geometrytype\(\w+\)\s*=\s*''(\w+)''', 'i'))[1]
                    FROM pg_catalog.pg_constraint AS con
                    WHERE
                        con.conrelid = cls.oid
                        AND attr.attnum = any(con.conkey)
                        AND pg_get_constraintdef(con.oid) ~* 'geometrytype\(\w+\)\s*=\s*''\w+'''
                    ORDER BY con.oid
                    LIMIT 1
                ),
                'GEOMETRY'
            ), 'ZM', ''), 'Z', '')
        END AS type -- noqa: RF04
    FROM pg_catalog.pg_attribute AS attr
    INNER JOIN pg_catalog.pg_class AS cls ON attr.attrelid = cls.oid
    INNER JOIN pg_catalog.pg_namespace AS ns ON cls.relnamespace = ns.oid
    INNER JOIN pg_catalog.pg_type AS tp ON attr.atttypid = tp.oid
    WHERE
        -- by type id, so the planner keeps the columns of every other type out of the join with pg_type
        attr.atttypid = any(array(
            SELECT geo.oid
            FROM pg_catalog.pg_type AS geo
            WHERE geo.typname IN ('geometry', 'geography')
        ))
        AND NOT attr.attisdropped
        AND cls.relkind IN ('r', 'v', 'm', 'f', 'p')
        AND NOT (tp.typname = 'geometry' AND cls.relname = 'raster_columns')
        AND NOT pg_is_other_temp_schema(cls.relnamespace)
        AND ($1::text[] IS NULL OR lower(ns.nspname) = any($1::text[]))
        -- checked only for the relation kinds above, not for every index and TOAST table
        AND CASE WHEN cls.relkind IN ('r', 'v', 'm', 'f', 'p') THEN has_table_privilege(cls.oid, 'SELECT') END
)

SELECT
    gc.schema,
    gc.name,
    gc.geom,
    gc.srid,
    gc.type,
    gc.relkind,
    cols.properties,
    cols.column_types,
    exists(
        -- a single-column spatial index on the geo column
        SELECT 1
        FROM pg_catalog.pg_index AS ix
        INNER JOIN pg_catalog.pg_opclass AS op ON ix.indclass[0] = op.oid
        WHERE
            ix.indrelid = gc.relid
            AND ix.indnkeyatts = 1
            AND ix.indkey[0] = gc.attnum
            AND op.opcname IN (
                'gist_geometry_ops_2d', 'spgist_geometry_ops_2d',
                'brin_geometry_inclusion_ops_2d',
                'gist_geography_ops'
            )
    ) AS geom_idx,
    (
        -- the comment on a table, view or materialized view
        SELECT d.description
        FROM pg_catalog.pg_description AS d
        WHERE
            d.objoid = gc.relid
            AND d.classoid = 'pg_catalog.pg_class'::regclass
            AND d.objsubid = 0
            AND gc.relkind IN ('r', 'v', 'm')
    ) AS description
FROM geo_columns AS gc
CROSS JOIN LATERAL (
    -- the table's other columns, by the type they are declared as and the type a query returns them as
    SELECT
        coalesce(
            jsonb_object_agg(attr.attname, trim(LEADING '_' FROM tp.typname))
            FILTER (WHERE trim(LEADING '_' FROM tp.typname) NOT IN ('geometry', 'geography')),
            '{}'::jsonb
        ) AS properties,
        coalesce(
            jsonb_object_agg(
                attr.attname,
                CASE
                    WHEN tp.typtype = 'd'
                        THEN (
                            SELECT domains.base_name
                            FROM domains
                            WHERE domains.domain_oid = tp.oid AND domains.base_typtype != 'd'
                        )
                    ELSE tp.typname
                END
            ),
            '{}'::jsonb
        ) AS column_types
    FROM pg_catalog.pg_attribute AS attr
    INNER JOIN pg_catalog.pg_type AS tp ON attr.atttypid = tp.oid
    WHERE
        attr.attrelid = gc.relid
        AND attr.attnum > 0
        AND NOT attr.attisdropped
        AND attr.attname != gc.geom
) AS cols;
