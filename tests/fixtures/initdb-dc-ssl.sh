#!/usr/bin/env sh
set -e

mv /var/lib/postgresql/data/pg_hba.conf /var/lib/postgresql/data/pg_hba.conf.bak
cat > /var/lib/postgresql/data/pg_hba.conf <<EOF
# TYPE  DATABASE        USER            ADDRESS                 METHOD

# "local" is for Unix domain socket connections only
#local   all             all                                     trust

# localhost connections
#host    all             all             127.0.0.1/32            trust

# external connections
hostssl all             all             all                     scram-sha-256

EOF
