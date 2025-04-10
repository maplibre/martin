drop table if exists trips_by_hour;
create table trips_by_hour as
select
    pulocationid,
    round(
        avg(extract(epoch from (dropoff_datetime - pickup_datetime)) / 60)
    )::INTEGER as trips_duration,
    count(*) as trips_count,
    round(avg(total_amount)) as trips_price,
    date_trunc('hour', pickup_datetime) as pickup_datetime
from trips
group by pulocationid, date_trunc('hour', pickup_datetime);
