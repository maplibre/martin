#!/usr/bin/env bash

for i in 01 02 03 04 05 06 07 08 09 10 11 12
do
    fileName=/data/yellow_tripdata_2017-${i}.csv

    if [[ ! -e ${fileName} ]]
    then
        wget -O ${fileName} "https://s3.amazonaws.com/nyc-tlc/trip+data/${fileName}"
        dos2unix ${fileName}
        sed -i -e '2d' ${fileName}
        dockerize -wait tcp://db:5432 -timeout 1m
        PGPASSWORD=postgres psql -U postgres -d db -h db -c "COPY trips FROM '${fileName}' WITH csv header;"
    fi
done
