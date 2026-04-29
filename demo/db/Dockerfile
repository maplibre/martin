FROM postgis/postgis:18-3.6@sha256:5703e4925db4f2bea02a5b12ee39e9ff483fe2bdfcbcd77a53d61494a86c785c

RUN set -x \
  && apt-get update \
  && apt-get install -y --no-install-recommends \
  wget \
  ca-certificates \
  unzip \
  dos2unix \
  gdal-bin \
  && rm -rf /var/lib/apt/lists/*
