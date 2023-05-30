drop table if exists trips_by_hour;
create table trips_by_hour as
select
	pulocationid,
	count(*) as trips_count,
	round(avg(total_amount)) trips_price,
	round(avg(extract(epoch from (dropoff_datetime - pickup_datetime)) / 60))::INTEGER trips_duration,
	date_trunc('hour', pickup_datetime) as pickup_datetime
from trips
group by pulocationid, date_trunc('hour', pickup_datetime);
