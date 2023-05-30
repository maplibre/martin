create extension postgis;
-- create extension timescaledb;

create table trips
(
  vendorid              numeric,
  pickup_datetime       timestamp,
  dropoff_datetime      timestamp,
  passenger_count       numeric,
  trip_distance         numeric,
  ratecodeid            numeric,
  store_and_fwd_flag    text,
  pulocationid          numeric,
  dolocationid          numeric,
  payment_type          numeric,
  fare_amount           numeric,
  extra                 numeric,
  mta_tax               numeric,
  tip_amount            numeric,
  tolls_amount          numeric,
  improvement_surcharge numeric,
  total_amount          numeric,
  congestion_surcharge  numeric,
  airport_fee           numeric
);

create function tilebbox(z integer, x integer, y integer, srid integer DEFAULT 3857) returns geometry
immutable
language plpgsql
as $$
declare
  max numeric := 20037508.34;
  res numeric := (max*2)/(2^z);
  bbox geometry;
begin
  bbox := ST_MakeEnvelope(
      -max + (x * res),
      max - (y * res),
      -max + (x * res) + res,
      max - (y * res) - res,
      3857
  );
  if srid = 3857 then
    return bbox;
  else
    return ST_Transform(bbox, srid);
  end if;
end;
$$
;

create or replace function get_trips(z integer, x integer, y integer, query_params json) returns bytea
    stable
    strict
    parallel safe
    language plpgsql
as $$
DECLARE
  bounds GEOMETRY(POLYGON, 3857) := TileBBox(z, x, y, 3857);
  date_from DATE := (query_params->>'date_from')::DATE;
  date_to DATE := (query_params->>'date_to')::DATE;
  in_hour INTEGER := (query_params->>'hour')::INTEGER;
  in_dow INTEGER[];
  res BYTEA;
BEGIN
  WITH sel_zones AS (
    SELECT locationid, geom
    FROM taxi_zones
    WHERE geom && bounds
  ),
  tile AS (
      SELECT
        sel_zones.locationid,
        coalesce(sum(trips_by_hour.trips_count), 0)::integer AS trips,
            coalesce(round(avg(trips_by_hour.trips_price)), 0)::integer AS trips_price,
            coalesce(round(avg(trips_by_hour.trips_duration)), 0)::integer AS trips_duration,
        ST_AsMVTGeom(min(sel_zones.geom), bounds, 4096, 1024, TRUE) AS geom
      FROM
          sel_zones LEFT JOIN trips_by_hour ON (
            sel_zones.locationid = trips_by_hour.pulocationid
            AND cast(trips_by_hour.pickup_datetime AS DATE) >= date_from
            AND cast(trips_by_hour.pickup_datetime AS DATE) <= date_to
            AND ((extract (HOUR FROM trips_by_hour.pickup_datetime) = in_hour) OR (in_hour = -1))
        )
  GROUP BY
        sel_zones.locationid
  )
  SELECT INTO res ST_AsMVT(tile, 'trips', 4096, 'geom')
  FROM tile
  WHERE geom IS NOT NULL;

  RETURN res;

END;
$$
;

alter function get_trips(integer, integer, integer, json) owner to postgres;
