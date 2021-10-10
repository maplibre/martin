SELECT
  ST_Transform (ST_SetSRID (ST_Extent ({geometry_column}), {srid}), 4326) AS bounds
FROM
  {id}
