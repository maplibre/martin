WITH real_bounds AS (SELECT ST_SetSRID(ST_Extent({geometry_column}), {srid}) AS rb FROM {schema}.{table})
SELECT ST_Transform(
               CASE
                   WHEN (SELECT ST_GeometryType(rb) FROM real_bounds LIMIT 1) = 'ST_Point'
                       THEN ST_SetSRID(ST_Extent(ST_Expand({geometry_column}, 1)), {srid})
                   ELSE (SELECT * FROM real_bounds)
                   END
           , 4326) AS bounds
FROM {schema}.{table};
