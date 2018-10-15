#!/bin/bash

wget -O ${HOME}/taxi_zones.zip https://s3.amazonaws.com/nyc-tlc/misc/taxi_zones.zip
unzip ${HOME}/taxi_zones.zip -d ${HOME}

for i in 01 02 03 04 05 06 07 08 09 10 11 12
do
    fileName=yellow_tripdata_2017-${i}.csv
    filePath=${HOME}/${fileName}

    wget -O ${filePath} "https://s3.amazonaws.com/nyc-tlc/trip+data/${fileName}"
    dos2unix ${filePath}
    sed -i -e '2d' ${filePath}
    psql -U postgres -d db -c "COPY trips FROM '${filePath}' WITH csv header;"
done
