FROM postgres:12

ENV PG_MAJOR=12
ENV POSTGIS_MAJOR=3

RUN set -x \
  && apt-get update \
  && apt-get install -y --no-install-recommends \
  postgresql-$PG_MAJOR-postgis-$POSTGIS_MAJOR \
  postgresql-$PG_MAJOR-postgis-$POSTGIS_MAJOR-scripts \
  wget \
  ca-certificates \
  unzip \
  dos2unix \
  gdal-bin \
  && rm -rf /var/lib/apt/lists/*
