FROM postgis/postgis:18-3.6@sha256:349dfb796455ef40d707f2f96972399718254074924525adec01fbae8b62a541

RUN set -x \
  && apt-get update \
  && apt-get install -y --no-install-recommends \
  wget \
  ca-certificates \
  unzip \
  dos2unix \
  gdal-bin \
  && rm -rf /var/lib/apt/lists/*
