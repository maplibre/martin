use actix::prelude::*;

pub struct CoordinatorActor;

impl Actor for CoordinatorActor {
  type Context = Context<Self>;

  fn started(&mut self, _ctx: &mut Self::Context) {
    info!("Starting CoordinatorActor");
  }
}
