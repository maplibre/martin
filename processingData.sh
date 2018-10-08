#!/usr/bin/env bash

mkdir -p data/
cd data/

for i in 01 02 03 04 05 06 07 08 09 10 11 12
do
    fileName=yellow_tripdata_2017-${i}.csv

    if [[ ! -e ${fileName} ]]
    then
        curl -o ${fileName} "https://s3.amazonaws.com/nyc-tlc/trip+data/${fileName}"
        dos2unix ${fileName}
        sed -i '/^$/d' ${fileName}
    fi
done

cd ..
