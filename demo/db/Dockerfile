FROM postgis/postgis:18-3.6@sha256:f248a10d133f63d01aefab324f3462d7e1002e9cc1b65c6585626f6cb7a3d85c

RUN set -x \
  && apt-get update \
  && apt-get install -y --no-install-recommends \
  wget \
  ca-certificates \
  unzip \
  dos2unix \
  gdal-bin \
  && rm -rf /var/lib/apt/lists/*
