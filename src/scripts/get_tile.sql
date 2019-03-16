WITH bounds AS (SELECT {mercator_bounds} as mercator, {original_bounds} as original)
SELECT ST_AsMVT(tile, '{id}', {extent}, 'geom' {id_column}) FROM (
  SELECT
    ST_AsMVTGeom({geometry_column_mercator}, bounds.mercator, {extent}, {buffer}, {clip_geom}) AS geom {properties}
  FROM {id}, bounds
  WHERE {geometry_column} && bounds.original
) AS tile WHERE geom IS NOT NULL