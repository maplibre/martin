WITH bounds AS (
  SELECT
    ST_Transform ({mercator_bounds}, {srid}) AS source
)
    SELECT
      ST_AsMVTGeom (ST_Transform ({geometry_column}, 3857), {mercator_bounds}, {extent}, {buffer}, {clip_geom}) AS geom {properties} FROM {id}, bounds
      WHERE
        {geometry_column} && bounds.source
