FROM postgis/postgis:18-3.6@sha256:1cd5da788ab0deddabefb607a51fcfcbcaf6ebc44ab917452ed9f8a529fc8e24

RUN set -x \
  && apt-get update \
  && apt-get install -y --no-install-recommends \
  wget \
  ca-certificates \
  unzip \
  dos2unix \
  gdal-bin \
  && rm -rf /var/lib/apt/lists/*
