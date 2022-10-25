#!/usr/bin/env sh
set -e

echo "Initializing docker-compose database"
/fixtures/initdb.sh
