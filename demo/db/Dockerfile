FROM postgis/postgis:18-3.6@sha256:6e6ed346296866e89ab9665d69f0aa6cd62f1ad62fec42e73adf397ab259e5d1

RUN set -x \
  && apt-get update \
  && apt-get install -y --no-install-recommends \
  wget \
  ca-certificates \
  unzip \
  dos2unix \
  gdal-bin \
  && rm -rf /var/lib/apt/lists/*
