use actix::{Actor, Addr, SyncArbiter, System, SystemRunner};
use actix_web::server;

use super::app;
use super::config::Config;
use super::coordinator_actor::CoordinatorActor;
use super::db::PostgresPool;
use super::db_executor::DbExecutor;

pub fn new(pool: PostgresPool, config: Config, watch_mode: bool) -> SystemRunner {
    let server = System::new("server");

    let db = SyncArbiter::start(3, move || DbExecutor(pool.clone()));
    let coordinator: Addr<_> = CoordinatorActor::default().start();

    let keep_alive = config.keep_alive;
    let worker_processes = config.worker_processes;
    let listen_addresses = config.listen_addresses.clone();

    let _addr = server::new(move || {
        app::new(
            db.clone(),
            coordinator.clone(),
            config.table_sources.clone(),
            config.function_sources.clone(),
            watch_mode,
        )
    })
    .bind(listen_addresses.clone())
    .unwrap_or_else(|_| panic!("Can't bind to {}", listen_addresses))
    .keep_alive(keep_alive)
    .shutdown_timeout(0)
    .workers(worker_processes)
    .start();

    server
}
