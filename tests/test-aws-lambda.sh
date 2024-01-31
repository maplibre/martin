#!/usr/bin/env bash
set -euo pipefail

EVENT=$(sam local generate-event apigateway http-api-proxy \
  | jq '.rawPath="/"|.requestContext.http.method="GET"')

# `sam build` will copy the _entire_ context to a temporary directory,
# so just give it the files we need
mkdir -p .github/files/lambda-layer/bin/
cp ${MARTIN_BIN:-target/debug/martin} .github/files/lambda-layer/bin/
cp ./tests/fixtures/pmtiles2/webp2.pmtiles .github/files/lambda-function/
sam build -t .github/files/lambda.yaml

echo "$EVENT" | sam local invoke -e - \
  | jq -ne 'input.statusCode==200'
