#!/bin/bash

wget -O ${HOME}/taxi_zones.zip https://martin-documentation-site.s3.us-west-2.amazonaws.com/taxi_zones/taxi_zones.zip
unzip ${HOME}/taxi_zones.zip -d ${HOME}

geomColumn=geom
ogr2ogr -f PostgreSQL PG:dbname=db -nln taxi_zones -nlt MULTIPOLYGON -lco GEOMETRY_NAME=${geomColumn} ${HOME}/taxi_zones.shp
psql -U postgres -d db -c "alter table taxi_zones alter column ${geomColumn} type geometry(multipolygon, 3857) using st_transform(${geomColumn}, 3857); \
  create index idx_taxi_zones_geom on taxi_zones using gist(${geomColumn});"

for i in 01 02 03 04 05 06 07 08 09 10 11 12
do
    fileName=yellow_tripdata_2017-${i}.csv
    filePath=${HOME}/${fileName}

    wget -O ${filePath} "https://martin-documentation-site.s3.us-west-2.amazonaws.com/taxi_trips/taxo/${fileName}"
    dos2unix ${filePath}
    echo "format data"
    sed -i -e '2d' ${filePath}
    echo "load into database"
    psql -U postgres -d db -c "COPY trips FROM '${filePath}' WITH csv header;"
done
