use actix::prelude::*;

pub struct WorkerActor;

impl Actor for WorkerActor {
  type Context = Context<Self>;

  fn started(&mut self, _ctx: &mut Self::Context) {
    info!("Starting WorkerActor");
  }
}
