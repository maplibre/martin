FROM postgis/postgis:18-3.6@sha256:8c1367a8a3c0d52835f0da36729ff7e6a072f30fb28fa89e991f8b923bd0cb00

RUN set -x \
  && apt-get update \
  && apt-get install -y --no-install-recommends \
  wget \
  ca-certificates \
  unzip \
  dos2unix \
  gdal-bin \
  && rm -rf /var/lib/apt/lists/*
