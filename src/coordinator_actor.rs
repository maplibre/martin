use actix::prelude::*;
use std::io;

use super::messages;
use super::source::Sources;
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

impl Handler<messages::Connect> for CoordinatorActor {
  type Result = Addr<Syn, WorkerActor>;

  fn handle(&mut self, msg: messages::Connect, _: &mut Context<Self>) -> Self::Result {
    info!("WorkerActor connected");
    self.workers.push(msg.addr.clone());
    msg.addr
  }
}

impl Handler<messages::RefreshSources> for CoordinatorActor {
  type Result = Result<Sources, io::Error>;

  fn handle(&mut self, msg: messages::RefreshSources, _: &mut Context<Self>) -> Self::Result {
    for worker in &self.workers {
      worker.do_send(messages::RefreshSources {
        sources: msg.sources.clone(),
      });
    }

    Ok(msg.sources)
  }
}
