#!/bin/sh
set -x
ARTIFACT_DIR=/data/builds/martin

echo "build martin binaries"
cd ..
cargo install martin --locked --no-default-features --features mbtiles,sprites,metrics,pmtiles,postgres,styles,webui
cd freebsd

echo "copy martin binaries to scripts dir"
cp /root/.cargo/bin/martin ./scripts/freebsd/stage/usr/local/libexec/martin/martin
cp /root/.cargo/bin/martin-cp ./scripts/freebsd/stage/usr/local/libexec/martin/martin-cp
chmod ugo+x ./scripts/freebsd/stage/usr/local/libexec/martin/martin
chmod ugo+x ./scripts/freebsd/stage/usr/local/libexec/martin/martin-cp


echo "create freebsd package"
pkg create -M ./scripts/freebsd/+MANIFEST -r ./scripts/freebsd/stage -p ./scripts/freebsd/pkg-plist

echo "deploy binary"
cp martin*.pgk $ARTIFACT_DIR/.