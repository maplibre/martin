# Cloud Optimized GeoTIFF File Sources

> [!WARNING]
>
> This feature is currently unstable and thus not included in the default build. Its behavior may change in patch releases.
>
> To experiment with it, [install Rust](<https://rust-lang.org/tools/install/>), and run this to download, compile, and install martin with the unstable feature:
>
> ```bash
> cargo install martin --features=unstable-cog
> ```
>
> It is unstable due to the limitations of our current implementation:
>
> - [`EPSG:3857`](<https://epsg.io/3857>) is not yet supported =\> [https://github.com/maplibre/martin/pull/1893](<https://github.com/maplibre/martin/pull/1893>)
>
> We welcome contributions to help stabilize this feature!

Martin supports serving raster sources such as local and remote [COG (Cloud Optimized GeoTIFF)](<https://cogeo.org/>) files.

## Supported color type and bits per sample

| color type | bits per sample | supported | status |
| --- | --- | --- | --- |
| rgb/rgba | 8 | ✅ |  |
| rgb/rgba | 16/32... | 🛠️ | working on |
| gray | 8/16/32... | 🛠️ | working on |

## Supported compression

- None
- LZW
- Deflate
- PackBits

## Run Martin with CLI to serve cog files

```bash
# Configured with a directory containing `*.tif` or `*.tiff` TIFF files.
martin /with/tiff/dir1 /with/tiff/dir2
# Configured with dedicated TIFF files, local or remote.
martin /path/to/target1.tif https://example.org/path/cog.tif
# Configured with a combination of directories and dedicated TIFF files.
martin /with/tiff/files /path/to/target1.tif /path/to/target2.tiff
# Configured with a remote prefix; every TIFF object under it becomes a source.
martin s3://bucket/imagery/
```

## Run Martin with configuration file

To add a COG in martin, simply add

```yml
# Cloud Optimized GeoTIFF File Sources
cog:
  # Interval between remote polls (HEAD checks and prefix re-listings). Defaults to "10m".
  # Set to "0s" to disable remote polling and remote-prefix discovery.
  reload_interval: 10m
  # Authentication, endpoint, and HTTP client settings are documented under "Remote COG" below.
  allow_http: true
  paths:
    # scan this whole dir, matching all *.tif and *.tiff files
    - /dir-path
    # specific TIFF file will be published as a cog source
    - /path/to/cog_file1.tif
    - /path/to/cog_file2.tiff
    # every TIFF object under this remote prefix becomes a cog source
    - s3://my-bucket/imagery/
  sources:
    # named source matching source name to a single file, local or remote
     cog-src1: /path/to/cog1.tif
     cog-src2: https://example.org/path/cog2.tif
```

## COG Hot Reload

Two mechanisms keep the catalog current at runtime - local directories are watched with filesystem events, remote COGs are polled.

### Local directories

When `.tif` or `.tiff` files are added, modified, or removed from a watched directory, Martin automatically updates the tile catalog - no restart required.

```yaml
cog:
  paths:
    - /path/to/cog/directory
```

> [!TIP]
>
> **Scanning subdirectories**
>
> Set `recursive: true` next to `paths` to scan subdirectories too. A nested file is named by its path relative to the scanned directory with `/` replaced by `.`, so `2024/roads.tif` becomes `2024.roads`.

> [!TIP]
>
> **Per-project directories**
>
> List a directory of project directories under `collections` to publish every file inside a project as `<project>.<file>`, so `/projects/tiles/project1/elevation.tif` becomes `project1.elevation`.

The following events are handled automatically:

- **File added** - the new source appears in the catalog.
- **File modified** - the source is reloaded and its tile cache is invalidated.
- **File removed** - the source is removed from the catalog.

### Remote COGs

Remote object stores and HTTP(S) servers have no event channel, so Martin polls them at `cog.reload_interval` (default `10m`):

- **Configured remote objects** - `cog.sources` entries with an `s3://`, `gs://`, `az://`, `http://`, or `https://` URL, as well as remote URLs passed on the CLI, are re-checked with a `HEAD` request and rebuilt when their `ETag` or `Last-Modified` changes.
- **Remote prefixes** - prefixes in `cog.paths` are re-listed, and the resulting objects are diffed against the previous snapshot, so added, updated, and removed TIFF objects propagate to the catalog.

Configured remote objects still load at startup when `reload_interval` is `0s`, but are not checked again. Remote prefixes are first discovered by the polling loop, so setting `reload_interval` to `0s` prevents their sources from loading.

If a later `HEAD` request or prefix listing fails, Martin retains the last-known object version or last successful listing, so a transient outage does not remove live sources. Before the first successful listing, a failed prefix is skipped for that poll and retried later. With `on_invalid: warn`, failed additions and replacements also remain pending for the next poll; an unsuccessful replacement keeps serving the last good source.

## Remote COG

Remote COGs are read with byte-range requests, so Martin fetches the TIFF metadata and image chunks it needs instead of downloading the complete object first. Only `.tif` and `.tiff` objects under a listed prefix are published, using the file stem as the initial source ID.

```yaml
cog:
  reload_interval: 1m
  endpoint: http://localhost:9000
  region: us-east-1
  allow_http: true
  access_key_id: ${AWS_ACCESS_KEY_ID}
  secret_access_key: ${AWS_SECRET_ACCESS_KEY}
  paths:
    - s3://my-bucket/imagery/
  sources:
    mosaic: s3://my-bucket/imagery/mosaic.tif
    raster: https://tiles.example.org/mosaic.tif
```

> [!NOTE]
>
> Local files configured directly in `cog.paths` or `cog.sources` are loaded at startup but are not watched for changes. Remote objects in either setting are polled, while remote prefixes in `cog.paths` are re-listed.

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

## About COG

[COG](<https://cogeo.org/>) is just Cloud Optimized GeoTIFF file.

TIFF is an image file format. TIFF tags are something like key-value pairs inside to describe the metadata about a TIFF file, ike `ImageWidth`, `ImageLength`, etc.

GeoTIFF is a valid TIFF file with a set of TIFF tags to describe the 'Cartographic' information associated with it.

COG is a valid GeoTIFF file with some requirements for efficient reading. That is, all COG files are valid GeoTIFF files, but not all GeoTIFF files are valid COG files. For quick access to tiles in TIFF files, Martin relies on the requirements/recommendations(like the [requirement about Reduced-Resolution Subfiles](<https://docs.ogc.org/is/21-026/21-026.html#_requirement_reduced_resolution_subfiles>) and [the content dividing strategy](<https://docs.ogc.org/is/21-026/21-026.html#_tiles>)) so we use the term `COG` over `GeoTIFF` in our documentation and configuration files.

You may want to visit these specs:

- [TIFF 6.0](<https://www.itu.int/itudoc/itu-t/com16/tiff-fx/docs/tiff6.pdf>)
- [GeoTIFF](<https://docs.ogc.org/is/19-008r4/19-008r4.html>)
- [Cloud Optimized GeoTIFF](<https://docs.ogc.org/is/21-026/21-026.html>)

### COG generation with GDAL

You could generate cog with `gdal_translate` or `gdalwarp`. See more details in [gdal doc](<https://gdal.org/en/latest/drivers/raster/cog.html>).

```bash
# gdal-bin installation
# sudo apt update
# sudo apt install gdal-bin

# gdalwarp
gdalwarp src1.tif src2.tif out.tif -of COG

# or gdal_translate
gdal_translate input.tif output_cog.tif -of COG
```

### The mapping from ZXY to tiff chunk

- A single TIFF file could contains many sub-file about same spatial area, each has different resolution
- A sub file is organized with many tiles

So basically there's a mapping from zxy to tile of sub-file of TIFF.

| zxy | mapping to |
| --- | --- |
| Zoom level | which sub-file in TIFF file |
| X and Y | which tile in subfile |

Clients could read only the header part of COG to figure out the mapping from zxy to the chunk number and the subfile number. Martin get tile to frontend by this mapping.
