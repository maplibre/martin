//! An S3-compatible object store in a `RustFS` container, filled over the S3 API.

use std::borrow::Cow;
use std::path::Path;
use std::time::Duration;

use backon::{ConstantBuilder, Retryable as _};
use object_store::aws::AmazonS3Builder;
use object_store::path::Path as ObjectPath;
use object_store::{ObjectStoreExt as _, PutPayload};
use testcontainers_modules::testcontainers::core::{CmdWaitFor, ExecCommand, WaitFor};
use testcontainers_modules::testcontainers::runners::AsyncRunner as _;
use testcontainers_modules::testcontainers::{ContainerAsync, Image};

/// The access key and secret of the container's only user.
const CREDENTIALS: &str = "rustfsadmin";

/// The `rustfs/rustfs` image.
///
/// It writes its startup logs to a file, not to stdout. The entrypoint prints
/// "Starting: /usr/bin/rustfs <dir>" just before exec-ing the server, which is waited for,
/// and then the server gets a few seconds to bind.
#[derive(Clone, Debug)]
struct RustFsImage;

impl Image for RustFsImage {
    fn name(&self) -> &'static str {
        "rustfs/rustfs"
    }

    fn tag(&self) -> &'static str {
        "1.0.0"
    }

    fn ready_conditions(&self) -> Vec<WaitFor> {
        vec![
            WaitFor::message_on_stdout("Starting: /usr/bin/rustfs"),
            WaitFor::seconds(3),
        ]
    }

    fn env_vars(&self) -> impl IntoIterator<Item = (impl Into<Cow<'_, str>>, impl Into<Cow<'_, str>>)> {
        [
            ("RUSTFS_ADDRESS", ":9000"),
            ("RUSTFS_ACCESS_KEY", CREDENTIALS),
            ("RUSTFS_SECRET_KEY", CREDENTIALS),
        ]
    }

    fn cmd(&self) -> impl IntoIterator<Item = impl Into<Cow<'_, str>>> {
        ["/data"]
    }
}

/// A running `RustFS` server, stopped when dropped.
pub struct RustFs {
    /// The container, kept alive for as long as the server is used.
    _container: ContainerAsync<RustFsImage>,
    /// The server's S3 endpoint, e.g. `http://127.0.0.1:32768`.
    endpoint: String,
}

impl std::fmt::Debug for RustFs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RustFs")
            .field("endpoint", &self.endpoint)
            .finish_non_exhaustive()
    }
}

impl RustFs {
    /// Start a `RustFS` server with an empty bucket for each of `buckets`.
    ///
    /// # Panics
    /// If Docker is not running or the container does not start.
    pub async fn start(buckets: &[&str]) -> Self {
        let container = (|| RustFsImage.start())
            .retry(
                ConstantBuilder::default()
                    .with_delay(Duration::from_secs(2))
                    .with_max_times(3),
            )
            .sleep(tokio::time::sleep)
            .await
            .expect("the RustFS container failed to start (is Docker running?)");
        for bucket in buckets {
            // RustFS serves each directory under `/data` as a bucket.
            container
                .exec(
                    ExecCommand::new(["mkdir", "-p", &format!("/data/{bucket}")])
                        .with_cmd_ready_condition(CmdWaitFor::exit()),
                )
                .await
                .expect("failed to create a bucket");
        }
        let host = container.get_host().await.expect("the RustFS container has no host");
        let port = container
            .get_host_port_ipv4(9000)
            .await
            .expect("the RustFS container does not expose port 9000");
        Self {
            _container: container,
            endpoint: format!("http://{host}:{port}"),
        }
    }

    /// Upload every file under `dir` into `bucket` over the S3 API, keyed by its path below `dir`.
    ///
    /// # Panics
    /// If a file cannot be read or uploaded.
    pub async fn upload_dir(&self, bucket: &str, dir: &Path) {
        let store = AmazonS3Builder::new()
            .with_endpoint(&self.endpoint)
            .with_bucket_name(bucket)
            .with_region("us-east-1")
            .with_access_key_id(CREDENTIALS)
            .with_secret_access_key(CREDENTIALS)
            .with_allow_http(true)
            .with_virtual_hosted_style_request(false)
            .build()
            .expect("failed to build the S3 client");
        let mut pending = vec![dir.to_owned()];
        while let Some(current) = pending.pop() {
            for entry in
                std::fs::read_dir(&current).unwrap_or_else(|e| panic!("failed to read {}: {e}", current.display()))
            {
                let path = entry.expect("failed to read a directory entry").path();
                if path.is_dir() {
                    pending.push(path);
                    continue;
                }
                let key = path
                    .strip_prefix(dir)
                    .expect("the walk stays inside the directory")
                    .to_str()
                    .expect("a fixture path is utf-8")
                    .replace(std::path::MAIN_SEPARATOR, "/");
                let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
                store
                    .put(&ObjectPath::from(key.as_str()), PutPayload::from(bytes))
                    .await
                    .unwrap_or_else(|e| panic!("failed to upload {key}: {e}"));
            }
        }
    }

    /// The server's S3 endpoint, e.g. `http://127.0.0.1:32768`, whose access key and secret are
    /// both `rustfsadmin`.
    #[must_use]
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }
}
