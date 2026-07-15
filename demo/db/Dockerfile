FROM postgis/postgis:18-3.6@sha256:c893f6f2652d11e13f50f8623045b3523991b41d038b4d213dc040f42641f0d7

RUN set -x \
  && apt-get update \
  && apt-get install -y --no-install-recommends \
  wget \
  ca-certificates \
  unzip \
  dos2unix \
  gdal-bin \
  && rm -rf /var/lib/apt/lists/*
