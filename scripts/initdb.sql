create extension postgis;
create extension timescaledb;
create table trips
(
  vendorid              numeric,
  tpep_pickup_datetime  timestamp,
  tpep_dropoff_datetime timestamp,
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

alter table trips
  owner to postgres;

create index pulocationid_idx
  on trips (pulocationid);
