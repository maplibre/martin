#!/bin/sh
ARTIFACT_DIR=/opt/builds/martin

if [ ! -d $ARTIFACT_DIR ]; then
  mkdir -p $ARTIFACT_DIR
fi

echo "build martin binaries"
cd ..
RUSTFLAGS="-Ctarget-cpu=native" cargo install martin --locked --no-default-features --features mbtiles,sprites,metrics,pmtiles,postgres,styles,webui
cd freebsd

echo "copy martin binaries to scripts dir"
mkdir -p ./scripts/freebsd/stage/usr/local/libexec/martin
cp /root/.cargo/bin/martin ./scripts/freebsd/stage/usr/local/libexec/martin/martin
cp /root/.cargo/bin/martin-cp ./scripts/freebsd/stage/usr/local/libexec/martin/martin-cp
chmod ugo+x ./scripts/freebsd/stage/usr/local/libexec/martin/martin
chmod ugo+x ./scripts/freebsd/stage/usr/local/libexec/martin/martin-cp

# get the martin version"
VERSION=$(grep "^version = " ../martin/Cargo.toml | sed "s/version = \"\(.*\)\"/\1/")
echo "martin version = $VERSION"

# update the martin version in the pkg manifest
sed -i '' -e "s/version\": \".*\"/version\": \"$VERSION\"/" ./scripts/freebsd/+MANIFEST

echo "create freebsd package"
pkg create -M ./scripts/freebsd/+MANIFEST -r ./scripts/freebsd/stage -p ./scripts/freebsd/pkg-plist

echo "deploy binary"
cp martin*.pkg $ARTIFACT_DIR/.
