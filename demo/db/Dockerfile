FROM postgis/postgis:18-3.6@sha256:9f9303f925b5da02c8b614a01b606041e863d001e415a098808002012fde0883

RUN set -x \
  && apt-get update \
  && apt-get install -y --no-install-recommends \
  wget \
  ca-certificates \
  unzip \
  dos2unix \
  gdal-bin \
  && rm -rf /var/lib/apt/lists/*
