FROM postgis/postgis:18-3.6@sha256:4a43e3c3e5d55b9208240fc71db543b79c638766280c625a425bd0a2390989c4

RUN set -x \
  && apt-get update \
  && apt-get install -y --no-install-recommends \
  wget \
  ca-certificates \
  unzip \
  dos2unix \
  gdal-bin \
  && rm -rf /var/lib/apt/lists/*
