FROM postgis/postgis:18-3.6@sha256:b4466ea2ba7e3af54fcbba996d45325720f4b3ecb4f43f99b2f41367dc196f23

RUN set -x \
  && apt-get update \
  && apt-get install -y --no-install-recommends \
  wget \
  ca-certificates \
  unzip \
  dos2unix \
  gdal-bin \
  && rm -rf /var/lib/apt/lists/*
