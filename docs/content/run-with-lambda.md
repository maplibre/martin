---
icon: fontawesome/brands/aws
tags:
  - deployment
  - aws
  - lambda
---

# Using with AWS Lambda

Martin can run in AWS Lambda.
This is useful if you want to serve tiles from a serverless environment, while accessing "nearby" data from a PostgreSQL database or PMTiles file in S3, without exposing the raw file to the world to prevent download abuse and improve performance.
The bucket can stay private.
Martin signs its S3 requests with the credentials Lambda gives the function, so the execution role only needs `s3:GetObject` on the archive.

Lambda has two deployment models: zip file and container-based. When using zip file deployment, there is an online code editor to edit the yaml configuration.
When using container-based deployment, we can pass our configuration on the command line or environment variables.

Everything can be performed via AWS CloudShell, or you can install the AWS CLI and the AWS SAM CLI, and configure authentication.
The CloudShell also runs in a particular AWS region.

### Container deployment

Lambda images must come from a public or private ECR registry. Pull the image from GHCR and push it to ECR.

```bash
$ docker pull ghcr.io/maplibre/martin:1.16.0 --platform linux/arm64
$ aws ecr create-repository --repository-name martin
[…]
        "repositoryUri": "493749042871.dkr.ecr.us-east-2.amazonaws.com/martin",

# Read the repositoryUri which includes your account number
$ docker tag ghcr.io/maplibre/martin:1.16.0 493749042871.dkr.ecr.us-east-2.amazonaws.com/martin:latest
$ aws ecr get-login-password --region us-east-2 \
  | docker login --username AWS --password-stdin 493749042871.dkr.ecr.us-east-2.amazonaws.com
$ docker push 493749042871.dkr.ecr.us-east-2.amazonaws.com/martin:latest
```

Open [Lambda console](https://console.aws.amazon.com/lambda) and create your function:

1. Click "Create function".
2. Choose "Container image".
3. Put something in "Function name".
   * **Note**: This is an internal identifier, not exposed in the function URL.
4. Click "Browse images", and select your repository and the tag.
   * If you cannot find it, see if you are in the same region?
5. Expand "Container image overrides", and under CMD put the URL of a `.pmtiles` file.
   An `s3://` URL works once the execution role may read the object, see the zip section below.
6. Set "Architecture" to `arm64` to match the platform that we pulled.
   Lambda has better ARM CPUs than x86.
7. Click "Create function".
8. Find the "Configuration" tab, select "Function URL", "Create function URL".
9. Set "Auth type" to `NONE`
   * Do not enable `CORS`. Martin already has `CORS` support, so it will create incorrect duplicate headers.
10. Click on the "Function URL".
11. To debug an issue, open the "Monitor" tab, "View CloudWatch logs", find the most recent Log stream.

### Zip deployment

It's possible to deploy the entire codebase from the AWS console, but we will use Serverless Application Model.
Our function will consist of a "Layer", containing the Martin binary, and our function itself will contain the configuration in yaml format.

#### The layer

Download the binary and place it in your staging directory.
The `bin` directory of your Layer will be added to the PATH.

```bash
mkdir -p martin_layer/src/bin/
cd martin_layer
curl -OL https://github.com/maplibre/martin/releases/latest/download/martin-aarch64-unknown-linux-musl.tar.gz
tar -C src/bin/ -xzf martin-aarch64-unknown-linux-musl.tar.gz martin
```

Every zip-based Lambda function runs a file called `bootstrap`.

```bash
cat <<EOF >src/bootstrap
#!/bin/sh
set -eu
exec martin --config \${_HANDLER}
EOF
```

Write the SAM template.

```yaml
cat <<EOF >template.yaml
AWSTemplateFormatVersion: 2010-09-09
Transform: 'AWS::Serverless-2016-10-31'
Resources:
  MartinLayer:
    Type: 'AWS::Serverless::LayerVersion'
    DeletionPolicy: Delete
    Properties:
      ContentUri: src
      CompatibleRuntimes:
      - provided.al2023
      CompatibleArchitectures:
      - arm64
Outputs:
  LayerArn:
    Value: !Ref MartinLayer
    Export:
      Name: !Sub "${AWS::StackName}-LayerArn"
EOF
```

Run `sam deploy --guided`.

1. Stack Name: Name your CloudFormation stack something like `martin-layer`.
2. Press enter for everything else
3. The settings are saved to `samconfig.toml`, so you can later do `sam deploy` to update the version, or `sam delete`.

Now if you visit the [Lambda console](https://console.aws.amazon.com/lambda/home) and select "Layers", you should see your layer.

#### The function

1. Select "Functions", "Create function".
2. Put something in "Function name".
3. Set "Runtime" to "Amazon Linux 2023".
4. Set "Architecture" to "arm64".
5. Under "Advanced settings", choose "Enable function URL" with "Auth type" of "NONE".
6. Click "Create function".

Add your layer:

1. Click "add a layer" (green banner at the top, or the very bottom).
2. Choose "Custom layers", and select your layer and its version.
3. Click "Add".

Add your configuration file in the function source code:

1. Code tab, File, New File: `config.yaml`.

   ```yaml
   pmtiles:
     sources:
       demotiles: s3://my-bucket/tiles.pmtiles
   # a new Lambda instance starts empty, so an in-process cache buys little
   cache:
     size_mb: 0
   ```

   A prefix such as `s3://my-bucket/tiles/` under `pmtiles.paths` serves every archive in it instead, which also needs `s3:ListBucket`.

2. Configuration tab, "General configuration", Edit: set "Handler" to `config.yaml`.
   The `bootstrap` above passes the handler name to `martin --config`.
3. Configuration tab, "Permissions": open the execution role and attach a policy that allows `s3:GetObject` on the archive.
   Nothing else is needed.
   Lambda hands the function temporary credentials through `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY` and `AWS_SESSION_TOKEN`, and Martin picks them up.
4. Click Deploy, wait for the success banner, and visit your function URL.

#### Everything as one SAM stack

The layer, the function, its role and the URL can be one template.
This is also how the steps above were verified.

```yaml
AWSTemplateFormatVersion: 2010-09-09
Transform: 'AWS::Serverless-2016-10-31'
Parameters:
  Bucket:
    Type: String
Resources:
  MartinLayer:
    Type: 'AWS::Serverless::LayerVersion'
    DeletionPolicy: Delete
    Properties:
      ContentUri: layer/
      CompatibleRuntimes:
        - provided.al2023
      CompatibleArchitectures:
        - arm64
  MartinFunction:
    Type: 'AWS::Serverless::Function'
    Properties:
      Runtime: provided.al2023
      Architectures:
        - arm64
      Layers:
        - !Ref MartinLayer
      CodeUri: function/
      Handler: config.yaml
      MemorySize: 512
      Timeout: 10
      FunctionUrlConfig:
        AuthType: NONE
      Policies:
        - S3ReadPolicy:
            BucketName: !Ref Bucket
Outputs:
  FunctionUrl:
    Value: !GetAtt MartinFunctionUrl.FunctionUrl
```

`layer/` holds `bootstrap` and `bin/martin`, `function/` holds `config.yaml`.
Deploy with `sam build && sam deploy --guided --parameter-overrides Bucket=my-bucket`.

### Sizing

At the Lambda defaults of 128 MB and 3 seconds, Martin v1.14.0 on `arm64` starts in about 300 ms, peaks at about 45 MB with one PMTiles source in S3, and answers a warm tile request in about 25 ms.
Raise the timeout if the archive is large or far away, and the memory if you serve many sources.

### Caching

Martin sets an `Etag` header on tile responses, and the `cache_control` option in the [configuration file](config-file/index.md) sets a default `Cache-Control` header.
Put CloudFront in front of the function URL and let it hold the tiles, since every new Lambda instance starts with an empty in-process cache.

### Not covered yet

* Connecting to a PostgreSQL database on RDS.
* A CloudFront distribution in front of the function URL, with the motivation and the basics.
