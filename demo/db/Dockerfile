FROM postgis/postgis:18-3.6@sha256:50bb646b2ebc5d67e039d9c52b9fe67602ab5a5b481699aa9b4874c781349aee

RUN set -x \
  && apt-get update \
  && apt-get install -y --no-install-recommends \
  wget \
  ca-certificates \
  unzip \
  dos2unix \
  gdal-bin \
  && rm -rf /var/lib/apt/lists/*
