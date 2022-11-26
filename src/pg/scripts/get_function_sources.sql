-- Find SQL functions that match these criteria:
--     * The function must have 3 or 4 input parameters,
--       first 3 must be integers and named z (or zoom), x, y (in that order),
--       with the optional JSON parameter as the 4th parameter (any name).
--     * The function output must be either a single bytea value or a table,
--       with the table row being either [bytea] or [bytea, text] (in that order).
--     * If the output is a two-column row, the second column will be used as etag (usually the MD5 hash)
--
-- Output fields:
--   schema: the schema the function is in
--   name: the function name
--   output_type: either "bytea" or "record"
--   output_record_types: an optional JSON array of parameter types ["bytea"] or ["bytea", "text"]
--   output_record_names: an optional JSON array of output column names, e.g. ["mvt", "key"]
--   input_names: a JSON array of input parameter names
--   input_types: a JSON array of input parameter types
WITH inputs AS (
    -- list of input parameters for each function, returned as a jsonb array [{name: type}, ...]
    SELECT specific_name,
           jsonb_agg(COALESCE(parameter_name::text, '_') ORDER BY ordinal_position) as input_names,
           jsonb_agg(data_type::text ORDER BY ordinal_position)                     as input_types
    FROM information_schema.parameters
    WHERE parameter_mode = 'IN'
    GROUP BY specific_name),
     outputs AS (
         -- list of output parameters for each function, returned as a jsonb array [{name: type}, ...]
         SELECT specific_name,
                jsonb_agg(data_type::text ORDER BY ordinal_position) as      out_params,
                jsonb_agg(parameter_name::text ORDER BY ordinal_position) as out_names
         FROM information_schema.parameters
         WHERE parameter_mode = 'OUT'
         GROUP BY specific_name)
SELECT routines.specific_schema AS schema,
       routines.routine_name    AS name,
       routines.data_type       AS output_type,
       outputs.out_params       AS output_record_types,
       out_names                AS output_record_names,
       inputs.input_types       AS input_types,
       inputs.input_names       AS input_names
FROM information_schema.routines
         JOIN inputs ON routines.specific_name = inputs.specific_name
         LEFT JOIN outputs ON routines.specific_name = outputs.specific_name
WHERE jsonb_array_length(input_names) IN (3, 4)
  AND input_names ->> 0 IN ('z', 'zoom')
  AND input_names ->> 1 = 'x'
  AND input_names ->> 2 = 'y'
  -- the 4th parameter is optional, and can be any name
  AND input_types ->> 0 = 'integer'
  AND input_types ->> 1 = 'integer'
  AND input_types ->> 2 = 'integer'
  AND (input_types ->> 3 = 'json' OR (input_types ->> 3) IS NULL)
  AND (
        (data_type = 'bytea' AND out_params IS NULL)
        OR (data_type = 'bytea' AND out_params = '["bytea"]'::jsonb)
        OR (data_type = 'record' AND out_params = '["bytea"]'::jsonb)
        OR (data_type = 'record' AND out_params = '["bytea", "text"]'::jsonb)
    )
ORDER BY routines.specific_schema, routines.routine_name;
