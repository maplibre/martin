create extension postgis;
create extension timescaledb;

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
  total_amount          numeric
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

-- TODO: add martin function
