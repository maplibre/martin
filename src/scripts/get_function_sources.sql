SELECT
  routines.specific_schema,
  routines.routine_name
FROM information_schema.routines
  LEFT JOIN information_schema.parameters ON routines.specific_name=parameters.specific_name
WHERE
  routines.data_type = 'bytea'
GROUP BY
  routines.specific_schema, routines.routine_name, routines.data_type
HAVING
  array_agg(array[parameters.parameter_name::text, parameters.data_type::text]) @>
  array[array['z', 'integer'], array['x', 'integer'], array['y', 'integer'], array['query_params', 'json']];