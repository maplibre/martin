FROM postgis/postgis:18-3.6@sha256:1ac8d723bbf66159010056348cb54f521a8ee4c83d8df7ccec0c103ca004c0b8

RUN set -x \
  && apt-get update \
  && apt-get install -y --no-install-recommends \
  wget \
  ca-certificates \
  unzip \
  dos2unix \
  gdal-bin \
  && rm -rf /var/lib/apt/lists/*
