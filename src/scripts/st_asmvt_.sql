with
  bounds as (
    select st_transform ({mercator_bounds}, {srid}) as srid
  ),
  source_1 as (
    select
      st_asmvt(tile, '{id}', {extent}, 'geom' {id_column})
    from (
      select
        st_asmvtgeom (st_transform ({geometry_column}, 3857), {mercator_bounds}, {extent}, {buffer}, {clip_geom}) as geom {properties}
      from
        {id}, bounds
      where
        {geometry_column} && bounds.srid) as tile
  )
  select
    source_1.tile
  from
    source_1;

with
    bounds as
    layer1 as (select '...' as tile),
    layer2 as (select '...' as tile),
    layer3 as (select '...' as tile)
        select
            layer1.tile || layer2.tile || layer3.tile
        from
            layer1,
            layer2
            layer3;

with
  bounds as (
    select
      st_transform({mercator_bounds}, {srid}) as srid
  ),
  tiles as (
    select
      '420' as tile1,
      '420' as tile2
  )
select
  tiles.tile1 || tiles.tile2
from
  tiles;

//

with
  bounds as (
    select st_transform({mercator_bounds}, {srid}) as srid)
select
  tiles.tile1 || tiles.tile2
from (
  select
    '420' as tile1,
    '69' as tile2
) as tiles;





------

WITH bounds AS (
  SELECT
    ST_Transform (ST_MakeEnvelope(-20037508.34, 20037508.34, 20037508.34, -20037508.34, 3857), 4326) AS source
)
SELECT
  ST_AsMVT (tile, 'public.points1', 4096, 'geom' ) FROM (
    SELECT
      ST_AsMVTGeom (ST_Transform (geom, 3857), ST_MakeEnvelope(-20037508.34, 20037508.34, 20037508.34, -20037508.34, 3857), 4096, 64, true) AS geom , "gid" FROM public.points1, bounds
      WHERE
        geom && bounds.source
) AS tile1

WITH bounds AS (
  SELECT
    ST_Transform (ST_MakeEnvelope(-20037508.34, 20037508.34, 20037508.34, -20037508.34, 3857), 4326) AS source
)
SELECT
  ST_AsMVT (tile, 'public.points2', 4096, 'geom' ) FROM (
    SELECT
      ST_AsMVTGeom (ST_Transform (geom, 3857), ST_MakeEnvelope(-20037508.34, 20037508.34, 20037508.34, -20037508.34, 3857), 4096, 64, true) AS geom , "gid" FROM public.points2, bounds
      WHERE
        geom && bounds.source
) AS tile









-- 0 CTE
-- (cost=516210.50..516210.55 rows=1 width=32) (actual time=631.428..631.567 rows=1 loops=1)
-- Planning Time: 1.518 ms
-- JIT:
--   Functions: 11
--   Options: Inlining true, Optimization true, Expressions true, Deforming true
--   Timing: Generation 2.829 ms, Inlining 46.263 ms, Optimization 144.057 ms, Emission 49.466 ms, Total 242.615 ms
-- Execution Time: 644.024 ms
-- TOTAL: 886ms?
WITH
  layer1 AS (SELECT ST_AsMVT(tile, 'public.points1', 4096, 'geom') FROM (SELECT ST_AsMVTGeom(ST_Transform(geom, 3857), ST_MakeEnvelope(-20037508.34, 20037508.34, 20037508.34, -20037508.34, 3857), 4096, 64, true) AS geom, "gid" FROM public.points1 WHERE geom && ST_Transform(ST_MakeEnvelope(-20037508.34, 20037508.34, 20037508.34, -20037508.34, 3857), 4326)) AS tile),
  layer2 AS (SELECT ST_AsMVT(tile, 'public.points2', 4096, 'geom') FROM (SELECT ST_AsMVTGeom(ST_Transform(geom, 3857), ST_MakeEnvelope(-20037508.34, 20037508.34, 20037508.34, -20037508.34, 3857), 4096, 64, true) AS geom, "gid" FROM public.points2 WHERE geom && ST_Transform(ST_MakeEnvelope(-20037508.34, 20037508.34, 20037508.34, -20037508.34, 3857), 4326)) AS tile)
SELECT
  layer1.ST_AsMVT || layer2.ST_AsMVT
FROM
  layer1, layer2;

-- 1 CTE
-- (cost=568.90..568.96 rows=1 width=32) (actual time=718.930..719.250 rows=1 loops=1)
-- Planning Time: 0.874 ms
-- Execution Time: 693.507 ms
WITH
  bounds AS (SELECT
    ST_Transform(ST_MakeEnvelope(-20037508.34, 20037508.34, 20037508.34, -20037508.34, 3857), 4326) AS source
  ),
  layer1 AS (SELECT ST_AsMVT(tile, 'public.points1', 4096, 'geom') as t FROM (SELECT ST_AsMVTGeom(ST_Transform(geom, 3857), ST_MakeEnvelope(-20037508.34, 20037508.34, 20037508.34, -20037508.34, 3857), 4096, 64, true) AS geom, "gid" FROM public.points1, bounds WHERE geom && bounds.source) AS tile),
  layer2 AS (SELECT ST_AsMVT(tile, 'public.points2', 4096, 'geom') as t FROM (SELECT ST_AsMVTGeom(ST_Transform(geom, 3857), ST_MakeEnvelope(-20037508.34, 20037508.34, 20037508.34, -20037508.34, 3857), 4096, 64, true) AS geom, "gid" FROM public.points2, bounds WHERE geom && bounds.source) AS tile)
SELECT
  layer1.t || layer2.t
FROM
  layer1, layer2;

-- 2 CTE
-- (cost=624.15..624.21 rows=1 width=32) (actual time=780.985..781.252 rows=1 loops=1)
-- Planning Time: 1.201 ms
-- Execution Time: 788.663 ms
WITH
  bounds AS (SELECT
    ST_MakeEnvelope(-20037508.34, 20037508.34, 20037508.34, -20037508.34, 3857) AS mercator,
    ST_Transform(ST_MakeEnvelope(-20037508.34, 20037508.34, 20037508.34, -20037508.34, 3857), 4326) AS source
  ),
  layer1 AS (SELECT ST_AsMVT(tile, 'public.points1', 4096, 'geom') as t FROM (SELECT ST_AsMVTGeom(ST_Transform(geom, 3857), bounds.mercator, 4096, 64, true) AS geom, "gid" FROM public.points1, bounds WHERE geom && bounds.source) AS tile WHERE geom IS NOT NULL),
  layer2 AS (SELECT ST_AsMVT(tile, 'public.points2', 4096, 'geom') as t FROM (SELECT ST_AsMVTGeom(ST_Transform(geom, 3857), bounds.mercator, 4096, 64, true) AS geom, "gid" FROM public.points2, bounds WHERE geom && bounds.source) AS tile WHERE geom IS NOT NULL)
SELECT
  layer1.t || layer2.t
FROM
  layer1, layer2;