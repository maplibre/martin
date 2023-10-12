DROP FUNCTION IF EXISTS public.function_zxy_raster;

CREATE OR REPLACE FUNCTION public.function_zxy_raster(z integer, x integer, y integer) RETURNS bytea AS $$
DECLARE
  mvt bytea;
BEGIN
  with rast as (
    SELECT
    st_clip(
        st_transform(st_union(rast),3857),
        st_tileenvelope(z,x,y) 
    )
    as bands
    from public.landcover where ST_ConvexHull(rast) && st_transform( st_tileenvelope(z,x,y),4326) 
  )
  SELECT into mvt ST_AsJPEG(st_tile(bands,256,256)) from rast;
  return mvt;
END
$$ LANGUAGE plpgsql IMMUTABLE STRICT PARALLEL SAFE;
