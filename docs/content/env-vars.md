---
icon: material/variable
tags:
  - configuration
  - deployment
---

# Environment Variables

Martin takes its configuration from [command line parameters](run-with-cli.md) and the [configuration file](config-file/index.md).
A configuration file can read any environment variable you name in it, for example `connection_string: ${DATABASE_URL}`.
See the [configuration section](config-file/index.md) for the substitution syntax.
Martin itself reacts to the following variables.

| Environment var                                                                                                                                                                                                                            | Description                                                                                                                                                                                                                                   |
|--------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|-----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `AWS_LAMBDA_RUNTIME_API`                                                                                                                                                                                                                   | If defined, connect to AWS Lambda to handle requests. The regular HTTP server is not used. See [Running in AWS Lambda](run-with-lambda.md)                                                                                                    |
| `AWS_CONTAINER_CREDENTIALS_RELATIVE_URI`<br/>`AWS_CONTAINER_CREDENTIALS_FULL_URI`<br/>`AWS_CONTAINER_AUTHORIZATION_TOKEN_FILE`<br/>`AWS_WEB_IDENTITY_TOKEN_FILE`<br/>`AWS_ROLE_ARN`<br/>`AWS_ROLE_SESSION_NAME`<br/>`AWS_ENDPOINT_URL_STS` | Injected by ECS, Fargate and EKS to say where the task role's credentials come from. Used for S3-backed PMTiles sources unless `pmtiles.profile` or the matching `pmtiles.*` setting is configured. See [PMTiles sources](sources-pmtiles.md) |
