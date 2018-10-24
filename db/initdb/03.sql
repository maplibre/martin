create table trips_by_hour as
select
    pulocationid,
    count(*) as trips_count,
    date_trunc('hour', pickup_datetime) as pickup_datetime
from trips
group by pulocationid, date_trunc('hour', pickup_datetime);