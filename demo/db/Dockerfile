FROM postgis/postgis:18-3.6@sha256:52d19e8b5a022824eb2e48a77ca448c1e47ab7081bd2bd9ff09e16b92e20326b

RUN set -x \
  && apt-get update \
  && apt-get install -y --no-install-recommends \
  wget \
  ca-certificates \
  unzip \
  dos2unix \
  gdal-bin \
  && rm -rf /var/lib/apt/lists/*
