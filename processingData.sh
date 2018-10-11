#!/usr/bin/env bash

for i in 01 02 03 04 05 06 07 08 09 10 11 12
do
    fileName=yellow_tripdata_2017-${i}.csv
    filePath=/data/${fileName}

    if [[ ! -e ${filePath} ]]
    then
        wget -O ${filePath} "https://s3.amazonaws.com/nyc-tlc/trip+data/${fileName}"
        dos2unix ${filePath}
        sed -i -e '2d' ${filePath}
        dockerize -wait tcp://db:5432 -timeout 1m
        PGPASSWORD=postgres psql -U postgres -d db -h db -c "COPY trips FROM '${filePath}' WITH csv header;"
    fi
done
