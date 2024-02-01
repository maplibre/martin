#!/usr/bin/env bash
set -euo pipefail

have () {
	hash -- "$1" 2>&-
}

if ! have sam; then
  echo "The AWS Serverless Application Model Command Line Interface (AWS SAM CLI) "
  echo "must be installed: https://docs.aws.amazon.com/serverless-application-model/latest/developerguide/install-sam-cli.html"
  exit 1
fi

# Just send a single request using `sam local invoke` to verify that
# the server boots, finds a source to serve, and can handle a request.
# TODO Run the fuller integration suite against this.
# In doing so, switch from `sam local invoke`, which starts and stops the
# server, to `sam local start-api`, which keeps it running.

EVENT=$(sam local generate-event apigateway http-api-proxy \
  | jq '.rawPath="/"|.requestContext.http.method="GET"')

# `sam build` will copy the _entire_ context to a temporary directory,
# so just give it the files we need
mkdir -p .github/files/lambda-layer/bin/
if ! install ${MARTIN_BIN:-target/debug/martin} .github/files/lambda-layer/bin/; then
  echo "Specify the binary, e.g. ‘MARTIN_BIN=target/x86_64-linux-unknown-musl/release/martin just test-lambda’"
  exit 1
fi
cp ./tests/fixtures/pmtiles2/webp2.pmtiles .github/files/lambda-function/
sam build -t .github/files/lambda.yaml

echo "$EVENT" | sam local invoke -e - \
  | jq -ne 'input.statusCode==200'
