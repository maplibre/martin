Remote files are read with byte-range requests, so Martin fetches only the metadata and tile or image chunks it needs instead of downloading the complete object first.
The same option names and credential-resolution rules apply to PMTiles and COG sources.
Place these options directly under the source kind's `pmtiles` or `cog` configuration section.

### HTTP(S)

An HTTP(S) URL can identify an individual file.
The supported schemes are `https://` and `http://`.
Plain `http://` URLs are refused unless `allow_http` is set to `true`.
This option can only be set in the configuration file, so a plain `http://` URL cannot be passed on the command line.
Prefer HTTPS outside trusted networks.

HTTP(S) URLs cannot be used for prefix discovery because Martin cannot enumerate an ordinary web directory.
Cloud-provider HTTPS endpoints are handled as ordinary HTTP URLs.
Use a provider-specific scheme below when Martin must apply cloud credentials or list a prefix.

### Amazon S3 and S3-compatible storage

The S3 backend also works with API-compatible providers such as [MinIO](https://www.min.io/), [Ceph](https://docs.ceph.com/en/latest/radosgw/s3/), [Cloudflare R2](https://developers.cloudflare.com/r2/), and [Hetzner Object Storage](https://www.hetzner.com/storage/object-storage/).

Use these provider-specific schemes for authenticated access and prefix listing:

- `s3://<bucket>/<path>`
- `s3a://<bucket>/<path>`

Public or presigned individual objects can also use standard HTTPS endpoint forms:

- `https://s3.<region>.amazonaws.com/<bucket>/<path>`
- `https://<bucket>.s3.<region>.amazonaws.com/<path>`
- `https://<account-id>.r2.cloudflarestorage.com/<bucket>/<path>`

A directly configured object requires `s3:GetObject`.
Discovering a prefix additionally requires `s3:ListBucket` on the bucket, scoped to that prefix where appropriate.

!!! tip "Provider-specific option names"
    Every S3 setting is also available with an `aws_` prefix, such as `aws_endpoint` and `aws_region`.
    Prefixes are useful when one source kind contains settings for multiple cloud providers.

#### Available Amazon S3 settings

--8<-- "object-store/aws.md"

### Google Cloud Storage

Use a `gs://<bucket>/<path>` URL for Google Cloud Storage.

!!! tip "Provider-specific option names"
    Every Google Cloud Storage setting is also available with a `google_` prefix.

#### Available Google Cloud Storage settings

--8<-- "object-store/google.md"

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

!!! tip "Provider-specific option names"
    Every Azure setting is also available with an `azure_` prefix.

#### Available Microsoft Azure settings

--8<-- "object-store/azure.md"

### HTTP client settings

The following security, connection, and proxy settings apply to HTTP(S) and the cloud backends above.

#### Available client settings

--8<-- "object-store/client.md"

### URLs, secrets, and saved configuration

Martin preserves URL query strings on object requests, so presigned and token-authenticated URLs work at runtime.
When it derives object URLs from a listed prefix, it retains the configured scheme, URL user information, host, custom port, query, and fragment.

For safety, errors and logs remove URL user information, query strings, and fragments.
`--save-config` also removes those URL components, cloud credentials, customer-provided encryption keys, and proxy user information.
A saved configuration therefore cannot retain a presigned URL token or inline credential; provide the secret again before restarting from the generated file.
Non-secret object-store settings retain their scalar types in saved configuration.
