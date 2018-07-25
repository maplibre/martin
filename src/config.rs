use num_cpus;
use serde_yaml;
use std::error::Error;
use std::fs::File;
use std::io;
use std::io::prelude::*;
use std::path::Path;

use super::db::PostgresConnection;
use super::source::{get_sources, Sources};

#[derive(Clone, Debug, Serialize)]
pub struct Config {
    pub pool_size: u32,
    pub keep_alive: usize,
    pub worker_processes: usize,
    pub listen_addresses: String,
    pub sources: Sources,
}

#[derive(Deserialize)]
pub struct ConfigBuilder {
    pub pool_size: Option<u32>,
    pub keep_alive: Option<usize>,
    pub worker_processes: Option<usize>,
    pub listen_addresses: Option<String>,
    pub sources: Sources,
}

impl ConfigBuilder {
    pub fn finalize(self) -> Config {
        Config {
            pool_size: self.pool_size.unwrap_or(20),
            keep_alive: self.keep_alive.unwrap_or(75),
            worker_processes: self.worker_processes.unwrap_or(num_cpus::get()),
            listen_addresses: self.listen_addresses.unwrap_or("0.0.0.0:3000".to_string()),
            sources: self.sources,
        }
    }
}

pub fn build(config_filename: &str, conn: PostgresConnection) -> io::Result<Config> {
    if Path::new(config_filename).exists() {
        info!("Config found at {}", config_filename);
        let config = read_config(config_filename)?;
        return Ok(config);
    };

    let sources = get_sources(conn)?;
    let config = generate_config(sources);

    // let _ = write_config(config_filename, config.clone());

    Ok(config)
}

pub fn generate_config(sources: Sources) -> Config {
    let config = ConfigBuilder {
        pool_size: None,
        keep_alive: None,
        worker_processes: None,
        listen_addresses: None,
        sources: sources,
    };

    config.finalize()
}

// pub fn write_config(file_name: &str, config: Config) -> io::Result<()> {
//     let mut file = File::create(file_name)?;
//     let yaml = serde_yaml::to_string(&config).unwrap();
//     file.write_all(yaml.as_bytes())?;
//     Ok(())
// }

pub fn read_config(file_name: &str) -> io::Result<Config> {
    let mut file = File::open(file_name)?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;

    let config: ConfigBuilder = serde_yaml::from_str(contents.as_str())
        .map_err(|err| io::Error::new(io::ErrorKind::Other, err.description()))?;

    Ok(config.finalize())
}
