# Environment Variables

Martin takes its configuration from [command line parameters](<https://maplibre.org/martin/run-with-cli/index.md>) and the [configuration file](<https://maplibre.org/martin/config-file/index.md>). A configuration file can read any environment variable you name in it, for example `connection_string: ${DATABASE_URL}`. See the [configuration section](<https://maplibre.org/martin/config-file/index.md>) for the substitution syntax. Martin itself reacts to the following variables.

| Environment var | Description |
| --- | --- |
| `AWS_LAMBDA_RUNTIME_API` | If defined, connect to AWS Lambda to handle requests. The regular HTTP server is not used. See [Running in AWS Lambda](<https://maplibre.org/martin/run-with-lambda/index.md>) |
| `AWS_CONTAINER_CREDENTIALS_RELATIVE_URI`<br>`AWS_CONTAINER_CREDENTIALS_FULL_URI`<br>`AWS_CONTAINER_AUTHORIZATION_TOKEN_FILE`<br>`AWS_WEB_IDENTITY_TOKEN_FILE`<br>`AWS_ROLE_ARN`<br>`AWS_ROLE_SESSION_NAME`<br>`AWS_ENDPOINT_URL_STS` | Injected by ECS, Fargate and EKS to say where the task role's credentials come from. Used for S3-backed PMTiles sources unless `pmtiles.profile` or the matching `pmtiles.*` setting is configured. See [PMTiles sources](<https://maplibre.org/martin/sources-pmtiles/index.md>) |
