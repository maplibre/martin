use actix::prelude::*;

use super::messages::Connect;
use super::worker_actor::WorkerActor;

pub struct CoordinatorActor {
  workers: Vec<Addr<Syn, WorkerActor>>,
}

impl Default for CoordinatorActor {
  fn default() -> CoordinatorActor {
    CoordinatorActor { workers: vec![] }
  }
}

impl Actor for CoordinatorActor {
  type Context = Context<Self>;

  fn started(&mut self, _ctx: &mut Self::Context) {
    info!("Starting CoordinatorActor");
  }
}

impl Handler<Connect> for CoordinatorActor {
  type Result = Addr<Syn, WorkerActor>;

  fn handle(&mut self, msg: Connect, _: &mut Context<Self>) -> Self::Result {
    info!("WorkerActor connected");
    self.workers.push(msg.addr.clone());
    msg.addr
  }
}
