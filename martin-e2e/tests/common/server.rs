//! Martin server lifecycle management for e2e tests
//!
//! This module provides utilities for starting, managing, and stopping
//! Martin server instances during e2e tests.

use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU16, Ordering};
use std::time::Duration;

use tokio::time::sleep;

use crate::common::Binaries;

/// A running Martin server instance for e2e testing.
///
/// The server is automatically stopped and cleaned up when dropped.
pub struct MartinServer {
    process: Child,
    url: String,
}

impl MartinServer {
    /// Start a Martin server with the given arguments.
    ///
    /// This will:
    /// 1. Spawn the martin binary as a child process
    /// 2. Wait for the server to become healthy
    /// 3. Return a MartinServer instance
    ///
    /// # Example
    ///
    /// ```no_run
    /// let server = MartinServer::start(&[
    ///     "tests/fixtures/mbtiles"
    /// ]).await.unwrap();
    /// ```
    pub async fn start(args: &[&str]) -> Result<Self, anyhow::Error> {
        static PORT: AtomicU16 = AtomicU16::new(3111);

        Self::start_with_port(args, PORT.fetch_add(1, Ordering::SeqCst)).await
    }

    /// Start a Martin server on a specific port.
    ///
    /// Useful when you need to run multiple servers in parallel.
    async fn start_with_port(args: &[&str], port: u16) -> Result<Self, anyhow::Error> {
        let url = format!("http://0.0.0.0:{port}");
        let listen_addr = format!("0.0.0.0:{port}");

        // Build full arguments with listen address
        let mut full_args = vec!["--listen-addresses", &listen_addr];
        full_args.extend_from_slice(args);

        eprintln!("Starting Martin server on {} with args: {:?}", url, args);

        let bins = Binaries::new();
        let process = Command::new(bins.martin)
            .args(&full_args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| anyhow::anyhow!("Failed to spawn martin process: {}", e))?;

        let mut server = Self { process, url };

        // Wait for server to be ready
        server.wait_for_health().await?;

        Ok(server)
    }

    /// Wait for the server to become healthy by polling the /health endpoint.
    async fn wait_for_health(&mut self) -> Result<(), anyhow::Error> {
        let health_url = format!("{}/health", self.url);
        let max_attempts = 60;
        let delay = Duration::from_millis(500);

        for attempt in 1..=max_attempts {
            // Check if process is still alive
            if let Ok(Some(status)) = self.process.try_wait() {
                anyhow::bail!("Martin process died with status {status}",);
            }

            // Try to connect to health endpoint
            match reqwest::get(&health_url).await {
                Ok(response) if response.status().is_success() => {
                    eprintln!("Martin server is healthy after {attempt} attempts",);
                    return Ok(());
                }
                Ok(response) => {
                    let status = response.status();
                    eprintln!("Health check attempt {attempt}/{max_attempts}: got status {status}");
                }
                Err(e) => {
                    eprintln!("Health check attempt {attempt}/{max_attempts}: {e}");
                }
            }

            sleep(delay).await;
        }

        anyhow::bail!("Martin server did not become healthy after {max_attempts} attempts",)
    }

    /// Make a GET request to the server.
    ///
    /// # Example
    ///
    /// ```no_run
    /// let response = server.get("/catalog").await;
    /// assert_eq!(response.status(), 200);
    /// ```
    pub async fn get(&self, path: &str) -> reqwest::Response {
        let url = format!("{}{}", self.url, path);
        reqwest::get(&url)
            .await
            .unwrap_or_else(|e| panic!("GET request to {} failed: {}", url, e))
    }

    /// Make a GET request and parse the response as JSON.
    pub async fn get_json(&self, path: &str) -> serde_json::Value {
        let response = self.get(path).await;
        if !response.status().is_success() {
            panic!(
                "GET {} failed with status {}: {}",
                path,
                response.status(),
                response.text().await.unwrap_or_default()
            );
        }
        response
            .json()
            .await
            .unwrap_or_else(|e| panic!("Failed to parse JSON from {}: {}", path, e))
    }

    /// Make a GET request and return the response bytes.
    pub async fn get_bytes(&self, path: &str) -> Vec<u8> {
        let response = self.get(path).await;
        if !response.status().is_success() {
            panic!("GET {} failed with status {}", path, response.status());
        }
        response
            .bytes()
            .await
            .unwrap_or_else(|e| panic!("Failed to read bytes from {}: {}", path, e))
            .to_vec()
    }

    /// Get the process ID of the Martin server.
    pub fn pid(&self) -> u32 {
        self.process.id()
    }
}

impl Drop for MartinServer {
    fn drop(&mut self) {
        eprintln!("Stopping Martin server (PID: {})", self.pid());
        self.process.kill().unwrap();
        self.process.wait().unwrap();
    }
}
