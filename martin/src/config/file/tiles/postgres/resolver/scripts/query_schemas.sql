-- Every user-visible schema in the database, used to tell "schema does not exist"
-- apart from "schema exists but has no tile-serving functions/tables".
SELECT nspname
FROM pg_namespace
WHERE
    nspname <> 'pg_catalog'
    AND nspname <> 'information_schema'
ORDER BY nspname;
