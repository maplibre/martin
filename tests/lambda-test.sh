#!/usr/bin/env bash
set -euo pipefail

EVENT=$(sam local generate-event apigateway http-api-proxy \
  | jq '.rawPath="/"|.requestContext.http.method="GET"')

sam build -t .github/files/lambda.yaml

echo "$EVENT" | sam local invoke \
  --parameter-overrides TAG="$TAG" -e - \
  | jq -ne 'input.statusCode==200'
