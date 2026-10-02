# PMTiles File Sources

Martin can serve any type of tiles from [PMTile](<https://protomaps.com/blog/pmtiles-v3-whats-new>) files. A PMTiles archive can be accessed either locally or remotely via HTTP range requests, e.g. from an object storage like S3. A path to a PMTiles file may be a URL. For example:

```bash
martin  /path/to/directory   https://example.org/path/tiles.pmtiles
```

You may also want to generate a [config file](<https://maplibre.org/martin/config-file/index.md>) using the `--save-config my-config.yaml`, and later edit it and use it with `--config my-config.yaml` option.

> [!TIP]
>
> See [MBTiles vs PMTiles](<https://maplibre.org/martin/sources-files/#mbtiles-vs-pmtiles>) for a comparison of the two file formats.

## PMTiles Hot Reload

Martin watches local directories configured under `pmtiles` for `.pmtiles` files using filesystem events, with the same add/modify/remove semantics described for [MBTiles](<https://maplibre.org/martin/sources-mbtiles/#mbtiles-hot-reload>).

```yaml
pmtiles:
  paths:
    - /path/to/pmtiles/directory
```

> [!TIP]
>
> **Scanning subdirectories**
>
> Set `recursive: true` next to `paths` to scan subdirectories too. A nested file is named by its path relative to the scanned directory with `/` replaced by `.`, so `2024/roads.pmtiles` becomes `2024.roads`.

> [!TIP]
>
> **Per-project directories**
>
> List a directory of project directories under `collections` to publish every file inside a project as `<project>.<file>`, so `/projects/tiles/project1/roads.pmtiles` becomes `project1.roads`. A collection is a local directory.

For remote object-storage prefixes (`s3://bucket/prefix/`, `gs://bucket/prefix/`, etc.) Martin periodically re-lists the prefix and diffs against the previous snapshot, taking into account object `ETag` or `Last-Modified` headers to detect updates to an existing source. There is no event channel from blob storage to subscribe to. Added, updated, and removed objects propagate to the catalog.

```yaml
pmtiles:
  paths:
    - s3://my-bucket/tiles/
  reload_interval: 10m  # default; set to "0s" to disable remote polling
```

> [!NOTE]
>
> Hot reload applies to directories and remote prefixes configured under `pmtiles.paths` (or passed on the CLI). Named sources listed under `pmtiles.sources` and individual remote-file URLs are snapshotted at startup and are not watched for changes.

## Serving PMTiles without a Tile Server

PMTiles archives can be served directly from HTTP range-capable storage without a dedicated tile server. This approach has several limitations:

- **Unrestricted access risk** Without proper access controls, clients may download large portions (or all) of an archive, leading to significant egress costs. A tile server restricts access to tile requests, but bulk extraction remains possible via many requests, which are generally easier to detect and block.
- **Over-fetching** PMTiles may fetch more data than strictly required per tile request to minimize the number of HTTP requests.
- **Lack of source composition** Direct serving does not support combining PMTiles with dynamic data sources (e.g., PostGIS) into a unified tile service. A tile server (e.g, Martin) is required for this.
- **Caching behavior** Cache efficiency may be reduced compared to setups with a dedicated tile server that can optimize request patterns.

## Serving PMTiles from local file systems, HTTP, or object storage

### Local files

Pass a path or `file://` URL on the command line:

```bash
martin path/to/tiles.pmtiles
```

Or configure a named source:

```yaml
pmtiles:
  sources:
    tiles: file:///path/to/tiles.pmtiles
```

### Remote files and prefixes

PMTiles supports HTTP(S), Amazon S3 and compatible services, Google Cloud Storage, and Microsoft Azure Storage. For example, configure a named S3 or S3-compatible source with:

```yaml
pmtiles:
  endpoint: http://localhost:9000
  region: us-east-1
  allow_http: true
  access_key_id: ${AWS_ACCESS_KEY_ID}
  secret_access_key: ${AWS_SECRET_ACCESS_KEY}
  sources:
    tiles: s3://my-bucket/tiles.pmtiles
```

Remote files are read with byte-range requests, so Martin fetches only the metadata and tile or image chunks it needs instead of downloading the complete object first. The same option names and credential-resolution rules apply to PMTiles and COG sources. Place these options directly under the source kind's `pmtiles` or `cog` configuration section.

### HTTP(S)

An HTTP(S) URL can identify an individual file. The supported schemes are `https://` and `http://`. Plain `http://` URLs are refused unless `allow_http` is set to `true`. This option can only be set in the configuration file, so a plain `http://` URL cannot be passed on the command line. Prefer HTTPS outside trusted networks.

HTTP(S) URLs cannot be used for prefix discovery because Martin cannot enumerate an ordinary web directory. Cloud-provider HTTPS endpoints are handled as ordinary HTTP URLs. Use a provider-specific scheme below when Martin must apply cloud credentials or list a prefix.

### Amazon S3 and S3-compatible storage

The S3 backend also works with API-compatible providers such as [MinIO](<https://www.min.io/>), [Ceph](<https://docs.ceph.com/en/latest/radosgw/s3/>), [Cloudflare R2](<https://developers.cloudflare.com/r2/>), and [Hetzner Object Storage](<https://www.hetzner.com/storage/object-storage/>).

Use these provider-specific schemes for authenticated access and prefix listing:

- `s3://<bucket>/<path>`
- `s3a://<bucket>/<path>`

Public or presigned individual objects can also use standard HTTPS endpoint forms:

- `https://s3.<region>.amazonaws.com/<bucket>/<path>`
- `https://<bucket>.s3.<region>.amazonaws.com/<path>`
- `https://<account-id>.r2.cloudflarestorage.com/<bucket>/<path>`

A directly configured object requires `s3:GetObject`. Discovering a prefix additionally requires `s3:ListBucket` on the bucket, scoped to that prefix where appropriate.

> [!TIP]
>
> **Provider-specific option names**
>
> Every S3 setting is also available with an `aws_` prefix, such as `aws_endpoint` and `aws_region`. Prefixes are useful when one source kind contains settings for multiple cloud providers.

#### Available Amazon S3 settings

> [!TIP]
>
> Next to the explicit configuration for auth and endpoints below, you can also use `profile` (for example `profile: staging`) to specify an [AWS SDK profile](<https://docs.aws.amazon.com/sdkref/latest/guide/file-format.html>). Explicit configuration overrides profiles.

> [!TIP]
>
> **Task roles on ECS, Fargate and EKS**
>
> Martin picks up the credential-discovery variables these runtimes inject (`AWS_CONTAINER_CREDENTIALS_RELATIVE_URI`, `AWS_CONTAINER_CREDENTIALS_FULL_URI`, `AWS_CONTAINER_AUTHORIZATION_TOKEN_FILE`, `AWS_WEB_IDENTITY_TOKEN_FILE`, `AWS_ROLE_ARN`, `AWS_ROLE_SESSION_NAME`, `AWS_ENDPOINT_URL_STS`) automatically, so task roles, IRSA and Pod Identity work without any of the settings below. Explicit configuration and `profile` override them. On EC2, the instance metadata service is used when nothing else is configured.

#### AWS specific Authentication &amp; Credentials

| configuration | description | example |
| --- | --- | --- |
| `access_key_id` | AWS Access Key | `AKIAIOSFODNN7EXAMPLE` |
| `secret_access_key` | Secret Access Key | `wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY` |
| `session_token`<br>`token` | AWS session token used for temporary credentials | `IQoJb3JpZ2luX2VjEOr...` |
| `web_identity_token_file` | Web identity token file path for AssumeRoleWithWebIdentity | `/var/run/secrets/eks.amazonaws.com/serviceaccount/token` |
| `role_arn` | Role ARN to assume with web identity token | `arn:aws:iam::123456789012:role/MyWebIdentityRole` |
| `role_session_name` | Session name for web identity assumption | `my-session` |
| `endpoint_url_sts` | Custom STS endpoint for web identity token exchange | `https://sts.amazonaws.com` |

#### AWS specific Connection &amp; Endpoint Configuration

| configuration | description | example |
| --- | --- | --- |
| `region` | AWS region<br>Defaults to `us-east-1` | `us-west-2` |
| `bucket`<br>`bucket_name` | Bucket name | `my-app-bucket` |
| `endpoint`<br>`endpoint_url` | Custom S3 endpoint<br>Defaults to regional endpoint<br>Ensure consistency with `virtual_hosted_style_request`<br>By default, **only HTTPS schemes are enabled**; enabling HTTP can expose sensitive data<br>Local testing example: `"http://localhost:4566"` | `https://s3.us-west-2.amazonaws.com` |
| `metadata_endpoint` | Instance metadata endpoint (IPv4 default `http://169.254.169.254`)<br>IPv6 alternative: `http://fd00:ec2::254` | `http://169.254.169.254` |
| `container_credentials_relative_uri` | ECS container credentials relative URI | `/v2/credentials/12345678-1234-1234-1234-123456789012` |
| `container_credentials_full_uri` | EKS container credentials full URI | `http://169.254.170.2/v2/credentials/abc123` |
| `container_authorization_token_file` | Authorization token file for EKS container creds | `/var/run/secrets/eks.amazonaws.com/serviceaccount/token` |

#### AWS specific Request Behavior &amp; Fallbacks

| configuration | description | example |
| --- | --- | --- |
| `imdsv1_fallback` | Fall back to IMDSv1 if IMDSv2 is **not supported** (useful for older kube2iam deployments)<br>**Security note:** AWS recommends IMDSv2 only; IMDSv1 is vulnerable to [SSRF attacks](<https://aws.amazon.com/blogs/security/defense-in-depth-open-firewalls-reverse-proxies-ssrf-vulnerabilities-ec2-instance-metadata-service/>)<br>Has no effect if not using instance credentials | `true` |
| `virtual_hosted_style_request` | Use virtual-hosted-style requests instead of path-style<br>Endpoint must match the style<br>Ensures correct bucket addressing for security and routing | `true` |
| `skip_signature` | Skip signing request<br>**Security warning:** Unsigned requests may expose credentials or allow tampering if endpoint is public | `true` |
| `unsigned_payload` | Use unsigned payload option (`UNSIGNED-PAYLOAD`) instead of signed<br>**Impact:** Checksums for request body are not computed in canonical requests, which can reduce integrity guarantees<br>Default is signed payload with checksum | `true` |
| `disable_tagging` | Disable tagging objects (useful if unsupported by backend) | `true` |
| `s3_express` | Enable S3 Express One Zone | `true` |
| `request_payer` | Enable S3 Requester Pays | `true` |

#### AWS Specific Object Integrity &amp; Encryption

| configuration | description | example |
| --- | --- | --- |
| `checksum_algorithm` | Checksum algorithm for uploads | `SHA256` |
| `server_side_encryption` | Type of server-side encryption:<br>`AES256` (SSE-S3)<br>`aws:kms` (SSE-KMS)<br>`aws:kms:dsse` (DSSE-KMS)<br>`sse-c` | `AES256` |
| `sse_kms_key_id` | KMS Key ID for SSE-KMS or DSSE-KMS | `arn:aws:kms:us-east-1:123456789012:key/abcd-1234-efgh-5678` |
| `sse_bucket_key_enabled` | Use bucket's default KMS key (`true`/`false`) | `true` |
| `sse_customer_key_base64` | Base64-encoded 256-bit key for SSE-C | `MDEyMzQ1Njc4OUFCQ0RFRjAxMjM0NTY3ODlBQkNERUY=` |

### Google Cloud Storage

Use a `gs://<bucket>/<path>` URL for Google Cloud Storage.

> [!TIP]
>
> **Provider-specific option names**
>
> Every Google Cloud Storage setting is also available with a `google_` prefix.

#### Available Google Cloud Storage settings

#### Google specific configuration

| configuration | description | example |
| --- | --- | --- |
| `base_url` | Sets the base URL for communicating with GCS.<br>If not explicitly set, it will be:<br>1\. Derived from the service account credentials, if provided<br>2\. Otherwise, uses the default GCS endpoint | `https://storage.googleapis.com` |
| `service_account`<br>`service_account_path` | Path to the service account file | `some/path/to/file` |
| `service_account_key` | The serialized service account key | `{"private_key": "private_key", "private_key_id": "private_key_id", "client_email":"client_email", "disable_oauth":true}` |
| `bucket`<br>`bucket_name` | Bucket name | `foobar-abc` |
| `application_credentials` | Set the path to the [application credentials file](<https://cloud.google.com/docs/authentication/provide-credentials-adc>) | `some/path/to/file` |
| `skip_signature` | Skip signing request | `true` |

### Microsoft Azure Storage

Use these provider-specific schemes for authenticated access and prefix listing:

- `abfs://<container>/<path>` and `abfss://<container>/<path>`
- `abfs://<file-system>@<account-name>.dfs.core.windows.net/<path>` and its `abfss://` form
- `az://<container>/<path>`
- `adl://<container>/<path>`
- `azure://<container>/<path>`

Public or presigned individual objects can also use Azure HTTPS endpoints:

- `https://<account>.dfs.core.windows.net/<container>/<path>`
- `https://<account>.blob.core.windows.net/<container>/<path>`
- the equivalent Microsoft Fabric DFS and Blob endpoints

> [!TIP]
>
> **Provider-specific option names**
>
> Every Azure setting is also available with an `azure_` prefix.

#### Available Microsoft Azure settings

#### Azure specific Authentication &amp; Credentials

| configuration | description | example |
| --- | --- | --- |
| `account_name` | Name of the Azure Storage account | `myaccount` |
| `access_key`<br>`account_key`<br>`master_key` | Master key for accessing the storage account<br><br>**Security note:**<br>Keep this key secret; anyone with access can read/write all data in the account | `abcd1234efgh5678ijkl9012mnop3456qrst7890uvwx1234yzab5678cdef9012` |
| `client_id` | Service principal client ID for OAuth authorization | `12345678-90ab-cdef-1234-567890abcdef` |
| `client_secret` | Service principal client secret for OAuth authorization<br>**Security note:** Must be kept confidential | `s3cr3tV@lu3!` |
| `tenant_id`<br>`authority_id` | Tenant ID used in OAuth flows | `abcdef12-3456-7890-abcd-ef1234567890` |
| `authority_host` | Authority host used in OAuth flows | `https://login.microsoftonline.com/` |
| `sas_key`<br>`sas_token` | Shared Access Signature (percent-encoded)<br><br>**Security note:**<br>Grants scoped access; treat as sensitive credentials | `sv=2021-06-08&ss=b&srt=sco&sp=rwdl`<br>`&se=2025-12-31T23:59:00Z&sig=ABCDEF1234567890` |
| `bearer_token`<br>`token` | Bearer token for requests<br><br>**Security note:**<br>Token must be protected; use HTTPS | `eyJ0eXAiOiJKV1QiLCJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9...` |
| `identity_endpoint`<br>`msi_endpoint` | Endpoint to request a managed identity token | `http://169.254.169.254/metadata/identity/oauth2/token` |
| `object_id` | Object ID for use with managed identity authentication | `12345678-90ab-cdef-1234-567890abcdef` |
| `msi_resource_id` | Resource ID for managed identity authentication | `/subscriptions/12345678-90ab-cdef-1234-567890abcdef/`<br>`resourcegroups/myrg/providers/Microsoft.ManagedIdentity/`<br>`userAssignedIdentities/myidentity` |
| `federated_token_file` | File containing token for Azure AD workload identity federation | `/var/run/secrets/azure/federated-token` |
| `use_azure_cli` | Use Azure CLI for acquiring access token | `true` |

#### Azure specific Connection &amp; Endpoint Configuration

| configuration | description | example |
| --- | --- | --- |
| `endpoint` | Override endpoint used to communicate with blob storage | `https://myaccount.blob.core.windows.net` |
| `object_store_use_emulator`<br>`use_emulator` | Use Azurite storage emulator | `true` |
| `use_fabric_endpoint` | Use Azure Fabric endpoint (account.dfs.fabric.microsoft.com) | `true` |
| `container_name` | Container name in the storage account | `mycontainer` |

#### Azure specific Request Behavior &amp; Security Options

| configuration | description | example |
| --- | --- | --- |
| `skip_signature` | Skip signing requests<br><br>**Security warning:**<br>Unsigned requests may expose sensitive data or allow tampering; use only in secure or local environments | `true` |
| `disable_tagging` | Disable object tagging (useful if backend does not support it) | `true` |
| `fabric_token_service_url` | URL of Fabric token service | `https://fabric-token.mycompany.com` |
| `fabric_workload_host` | Host for Fabric workload | `https://workload.fabric.mycompany.com` |
| `fabric_session_token` | Session token for Fabric<br><br>**Security note:** Must be protected; use HTTPS | `eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9...` |
| `fabric_cluster_identifier` | Identifier for Fabric cluster | `fabric-cluster-01` |

### HTTP client settings

The following security, connection, and proxy settings apply to HTTP(S) and the cloud backends above.

#### Available client settings

#### Security options

| configuration | description | example |
| --- | --- | --- |
| `allow_http` | Allow non-TLS, i.e. non-HTTPS connections<br><br>**Security warning:**<br>If you enable this option, attackers may be able to read the data you request<br><br>Defaults to `false` | `true` |
| `allow_invalid_certificates` | Skip certificate validation on https connections<br><br>**Security warning:**<br>You should think very carefully before using this method. If invalid certificates are trusted, any certificate for any site will be trusted for use. This includes expired certificates. This introduces significant vulnerabilities, and should only be used as a last resort or for testing | `true` |

#### Connection options

| configuration | description | example |
| --- | --- | --- |
| `user_agent` | User-Agent header to be used by this client | `martin 1.0.0` |
| `randomize_addresses` | Randomize order addresses that the DNS resolution yields.<br>This will spread the connections across more servers. | `true` |
| `connect_timeout` | Timeout for only the connect phase of a Client | `5s` |
| `read_timeout` | Timeout for each read operation, reset after every successful read.<br>Useful for detecting stalled connections when the response size is unknown.<br>Disabled by default; timeout errors are retried | `30s` |
| `timeout` | The timeout is applied from when the request starts connecting until the response body has finished | `10s` |
| `pool_idle_timeout` | The pool max idle timeout | `5m` |
| `pool_max_idle_per_host` | maximum number of idle connections per host | `10` |
| `http1_only` | Only use http1 connections | `false` |
| `http2_only` | Only use http2 connections | `false` |
| `http2_keep_alive_interval` | Interval for HTTP2 Ping frames should be sent to keep a connection alive. | `15s` |
| `http2_keep_alive_timeout` | Timeout for receiving an acknowledgement of the keep-alive ping. | `15s` |
| `http2_keep_alive_while_idle` | Enable HTTP2 keep alive pings for idle connections | `true` |
| `http2_max_frame_size` | Sets the maximum frame size to use for HTTP2. |  |

#### Proxy settings

| configuration | description | example |
| --- | --- | --- |
| `proxy_url` | HTTP proxy to use for requests | `http://proxy.example.com:8080` |
| `proxy_ca_certificate` | PEM-formatted CA certificate for proxy connections | `-----BEGIN CERTIFICATE-----`<br>...<br>`-----END CERTIFICATE-----` |
| `proxy_excludes` | List of hosts that bypass proxy | `example.com`, `maplibre.org` |

### URLs, secrets, and saved configuration

Martin preserves URL query strings on object requests, so presigned and token-authenticated URLs work at runtime. When it derives object URLs from a listed prefix, it retains the configured scheme, URL user information, host, custom port, query, and fragment.

For safety, errors and logs remove URL user information, query strings, and fragments. `--save-config` also removes those URL components, cloud credentials, customer-provided encryption keys, and proxy user information. A saved configuration therefore cannot retain a presigned URL token or inline credential; provide the secret again before restarting from the generated file. Non-secret object-store settings retain their scalar types in saved configuration.
