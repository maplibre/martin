FROM postgis/postgis:18-3.6@sha256:db37d16842d4518262e0722c0c4a136f3a54001edba13a4edfdcc9ce97f3a793

RUN set -x \
  && apt-get update \
  && apt-get install -y --no-install-recommends \
  wget \
  ca-certificates \
  unzip \
  dos2unix \
  gdal-bin \
  && rm -rf /var/lib/apt/lists/*
