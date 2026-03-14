FROM postgis/postgis:18-3.6@sha256:0544d3f21f1e069f8ed506bfdaf44b5bed8a1c317614dc7df2ac6c0f6f911ccd

RUN set -x \
  && apt-get update \
  && apt-get install -y --no-install-recommends \
  wget \
  ca-certificates \
  unzip \
  dos2unix \
  gdal-bin \
  && rm -rf /var/lib/apt/lists/*
