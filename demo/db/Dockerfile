FROM postgis/postgis:18-3.6@sha256:8d67cc8fe5f45808d54fe95cc210b05ce6b3ea3682e9a97c36362f3e1b8ff939

RUN set -x \
  && apt-get update \
  && apt-get install -y --no-install-recommends \
  wget \
  ca-certificates \
  unzip \
  dos2unix \
  gdal-bin \
  && rm -rf /var/lib/apt/lists/*
