## Using with AWS Lambda

Martin can be run in AWS Lambda. This is useful if you want to serve tiles from a serverless environment, while accessing "nearby" data from a PostgreSQL database or PMTiles file in S3, without exposing the raw file to the world to prevent download abuse and improve performance.

Some very brief context: Lambda has two deployment models, zip file and container-based. When using zip file deployment, the online code editor is available, in which we can edit the .yaml configuration. When using container-based deployment, we can pass our configuration on the command line or environment variables.

Everything can be performed from AWS CloudShell, otherwise you will need to install the AWS CLI and the AWS SAM CLI, and configure authentication. The CloudShell also runs in a particular AWS region.

### Container deployment

Lambda images must come from a public or private ECR registry. Pull the image from GHCR and push it to ECR.

```bash
$ docker pull ghcr.io/maplibre/martin:latest --platform linux/arm64
$ aws ecr create-repository --repository-name martin
[…]
        "repositoryUri": "493749042871.dkr.ecr.us-east-2.amazonaws.com/martin",
# Read the repositoryUri which includes your account number
$ docker tag ghcr.io/maplibre/martin:latest 493749042871.dkr.ecr.us-east-2.amazonaws.com/martin:latest
$ aws ecr get-login-password --region us-east-2 | docker login --username AWS --password-stdin 493749042871.dkr.ecr.us-east-2.amazonaws.com
$ docker push 493749042871.dkr.ecr.us-east-2.amazonaws.com/martin:latest
```

Now you can go to the [Lambda console](https://console.aws.amazon.com/lambda) and create your function.

1. Click “Create function”.
2. Choose “Container image”.
3. Put something in “Function name”. (Note: This is an internal identifier, not exposed in the function URL.)
4. Click “Browse images”, and select your repository and the tag. (If you can’t find it, see if you’re in the same region?)
5. Expand “Container image overrides”, and under CMD put the URL of a .pmtiles file.
6. Set “Architecture” to arm64 to match the platform that we pulled. (Lambda has better ARM CPUs than x86.)
7. Click “Create function”.
8. Find the “Configuration” tab, select “Function URL”, “Create function URL”.
9. Set “Auth type” to `NONE`
   * Do not enable CORS. Martin already has CORS support, so it will create duplicate headers and break CORS.
10. Click on the “Function URL”. If it works, hooray! If it doesn’t, open the “Monitor” tab, “View CloudWatch logs”, find the most recent Log stream.

### Zip deployment

It’s possible to deploy the entire codebase from the AWS console, but we will use Serverless Application Model. Our function will consist of a “Layer”, containing the Martin binary, and our function itself will contain the configuration in .yaml format.

#### The layer

Download the binary and place it in your staging directory. The `bin` directory of your Layer will be added to the PATH.

```bash
mkdir -p martin_layer/src/bin/
cd martin_layer
curl -OL https://github.com/maplibre/martin/releases/download/VERSION_NUMBER_HERE/martin-aarch64-unknown-linux-musl.tar.gz
tar -C src/bin/ -xzf martin-aarch64-unknown-linux-musl.tar.gz martin
```

Every zip-based Lambda function runs a file called `bootstrap`.

```bash
cat <<EOF >src/bootstrap
#!/bin/sh
set -eu
exec martin -c ${_HANDLER}.yaml
EOF
```

Write the SAM template.

```yaml
cat <<EOF >template.yaml
AWSTemplateFormatVersion: 2010-09-09
Transform: 'AWS::Serverless-2016-10-31'
Resources:
  martin:
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

Now if you visit the [Lambda console](https://console.aws.amazon.com/lambda/home) and select “Layers”, you should see your layer.

#### The function

1. Select “Functions”, “Create function”.
2. Put something in “Function name”.
3. Set “Runtime” to “Amazon Linux 2023”.
4. Set “Architecture” to “arm64”.
5. Under “Advanced settings”, choose “Enable function URL” with “Auth type” of “NONE”.
6. Click “Create function”.

Add your layer:

1. Click “add a layer” (green banner at the top, or the very bottom).
2. Choose “Custom layers”, and select your layer and its version.
3. Click “Add”.

Add your configuration file in the function source code:

1. Code tab, File, New File: `hello.handler.yaml`.

   ```yaml
   pmtiles:
     sources:
       demotiles: <url to a pmtiles file>
   ```

2. Click Deploy, wait for the success banner, and visit your function URL.

### TODO

This support is preliminary; there are features to add to Martin, configuration to tweak, and documentation to write.

- Lambda has a default timeout of 3 seconds, and 128 MB of memory, maybe this is suboptimal.
- Document how to connect to a PostgreSQL database on RDS.
- Set up a CloudFront CDN, this is a whole thing, but explain the motivation and the basics.
- Grant the execution role permission to read objects from an S3 bucket, and teach Martin how to make authenticated requests to S3.
- Teach Martin how to serve all PMTiles files from an S3 bucket rather than having to list them at startup.
- Teach Martin how to set the Cache-Control and Etag headers for better defaults.
