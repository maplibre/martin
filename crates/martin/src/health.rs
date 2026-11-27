//! `martin health`: probes a running Martin's `/health` endpoint, for container images without a shell or `wget`.

use std::time::Duration;

use reqwest::StatusCode;
use url::Url;

/// Health endpoint the official Docker image probes when no `--url` is given.
const DEFAULT_HEALTH_URL: &str = "http://127.0.0.1:3000/health";

/// How long the probe may take, from connecting to reading the response.
const TIMEOUT: Duration = Duration::from_secs(5);

/// Arguments of `martin health`.
#[derive(clap::Args, Debug, PartialEq, Eq)]
pub struct HealthArgs {
    /// URL of the health endpoint, for a Martin listening somewhere other than the default `--listen-addresses`
    #[arg(long, env = "MARTIN_HEALTH_URL", default_value = DEFAULT_HEALTH_URL)]
    pub url: Url,
}

/// Why the health probe failed.
#[derive(thiserror::Error, Debug)]
pub enum HealthError {
    /// The single-threaded runtime the probe runs on could not start.
    #[error("cannot start the async runtime")]
    Runtime {
        /// What failed.
        #[source]
        error: std::io::Error,
    },
    /// Building the client, connecting, sending or receiving failed.
    #[error("cannot reach {url}")]
    Request {
        /// The URL probed.
        url: Url,
        /// What failed.
        #[source]
        error: reqwest::Error,
    },
    /// The server answered with a status other than 2xx.
    #[error("{url} answered with status {status}")]
    Status {
        /// The URL probed.
        url: Url,
        /// The status the server sent.
        status: StatusCode,
    },
}

/// Sends one GET to the health URL, bypassing any proxy, and succeeds on a 2xx status.
pub fn check(args: &HealthArgs) -> Result<(), HealthError> {
    let url = &args.url;
    let request_error = |error| HealthError::Request {
        url: url.clone(),
        error,
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
        .map_err(|error| HealthError::Runtime { error })?;
    let status = runtime.block_on(async {
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(TIMEOUT)
            .user_agent("martin-health")
            .build()?;
        Ok::<_, reqwest::Error>(client.get(url.clone()).send().await?.status())
    });
    let status = status.map_err(request_error)?;
    if status.is_success() {
        Ok(())
    } else {
        Err(HealthError::Status {
            url: url.clone(),
            status,
        })
    }
}
