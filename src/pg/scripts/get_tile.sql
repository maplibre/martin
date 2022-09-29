SELECT
  ST_AsMVT (tile, '{id}', {extent}, 'geom' {id_column}) FROM ({geom_query}) AS tile
