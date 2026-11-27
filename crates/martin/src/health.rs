//! `martin health`: probes a running Martin's `/health` endpoint, for container images without a shell or `wget`.

use std::io::{Read as _, Write as _};
use std::net::TcpStream;
use std::time::Duration;

use url::{Position, Url};

/// Health endpoint the official Docker image probes when no `--url` is given.
const DEFAULT_HEALTH_URL: &str = "http://127.0.0.1:3000/health";

/// How long the probe waits to connect, and then for each read or write.
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
    /// Only plain HTTP is probed, as Martin serves no TLS itself.
    #[error("unsupported URL scheme {scheme}, only http is supported")]
    Scheme {
        /// The scheme the URL names.
        scheme: String,
    },
    /// Resolving, connecting, writing or reading failed.
    #[error("cannot reach {url}")]
    Io {
        /// The URL probed.
        url: Url,
        /// What failed.
        #[source]
        error: std::io::Error,
    },
    /// The server answered without a status line.
    #[error("{url} sent no HTTP status line")]
    NoStatus {
        /// The URL probed.
        url: Url,
    },
    /// The server answered with a status other than 2xx.
    #[error("{url} answered with status {status}")]
    Status {
        /// The URL probed.
        url: Url,
        /// The status code the server sent.
        status: String,
    },
}

/// Sends one GET to the health URL and succeeds on a 2xx status.
pub fn check(args: &HealthArgs) -> Result<(), HealthError> {
    let url = &args.url;
    if url.scheme() != "http" {
        return Err(HealthError::Scheme {
            scheme: url.scheme().to_owned(),
        });
    }
    let io = |error| HealthError::Io {
        url: url.clone(),
        error,
    };

    let mut last_error = std::io::Error::other("host resolved to no address");
    let mut stream = None;
    for addr in url.socket_addrs(|| None).map_err(io)? {
        match TcpStream::connect_timeout(&addr, TIMEOUT) {
            Ok(s) => {
                stream = Some(s);
                break;
            }
            Err(e) => last_error = e,
        }
    }
    let mut stream = stream.ok_or_else(|| io(last_error))?;
    stream.set_read_timeout(Some(TIMEOUT)).map_err(io)?;
    stream.set_write_timeout(Some(TIMEOUT)).map_err(io)?;

    let host = &url[Position::BeforeHost..Position::AfterPort];
    let path = &url[Position::BeforePath..Position::AfterQuery];
    let request =
        format!("GET {path} HTTP/1.1\r\nHost: {host}\r\nUser-Agent: martin-health\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).map_err(io)?;

    let mut head = [0_u8; 64];
    let mut len = 0;
    while len < head.len() && !head[..len].contains(&b'\n') {
        let n = stream.read(&mut head[len..]).map_err(io)?;
        if n == 0 {
            break;
        }
        len += n;
    }
    let line = String::from_utf8_lossy(&head[..len]);
    let status = line
        .lines()
        .next()
        .filter(|l| l.starts_with("HTTP/"))
        .and_then(|l| l.split_whitespace().nth(1))
        .ok_or_else(|| HealthError::NoStatus { url: url.clone() })?;
    if status.len() == 3 && status.starts_with('2') {
        Ok(())
    } else {
        Err(HealthError::Status {
            url: url.clone(),
            status: status.to_owned(),
        })
    }
}
