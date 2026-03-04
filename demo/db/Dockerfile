FROM postgis/postgis:18-3.6@sha256:4b7d734493b38e0ecf5e8d063446c96487cd11e2f75d2e52b95c0b03780143d6

RUN set -x \
  && apt-get update \
  && apt-get install -y --no-install-recommends \
  wget \
  ca-certificates \
  unzip \
  dos2unix \
  gdal-bin \
  && rm -rf /var/lib/apt/lists/*
