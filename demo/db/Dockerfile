FROM postgis/postgis:18-3.6@sha256:7ba2120137b73f16aadfa47b425014e08ed502b9641bbd42d457e0f8da25fc59

RUN set -x \
  && apt-get update \
  && apt-get install -y --no-install-recommends \
  wget \
  ca-certificates \
  unzip \
  dos2unix \
  gdal-bin \
  && rm -rf /var/lib/apt/lists/*
