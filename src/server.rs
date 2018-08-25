use actix::{SyncArbiter, System, SystemRunner};
use actix_web::server;

use super::app;
use super::config::Config;
use super::db::PostgresPool;
use super::db_executor::DbExecutor;

pub fn new(config: Config, pool: PostgresPool) -> SystemRunner {
    let server = System::new("server");
    let db_sync_arbiter = SyncArbiter::start(3, move || DbExecutor(pool.clone()));

    let keep_alive = config.keep_alive;
    let worker_processes = config.worker_processes;
    let listen_addresses = config.listen_addresses.clone();

    let _addr = server::new(move || {
        app::new(
            db_sync_arbiter.clone(),
            config.table_sources.clone(),
            config.function_sources.clone(),
        )
    }).bind(listen_addresses.clone())
        .expect(&format!("Can't bind to {}", listen_addresses))
        .keep_alive(keep_alive)
        .shutdown_timeout(0)
        .workers(worker_processes)
        .start();

    server
}
