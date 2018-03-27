use actix::prelude::*;
use std::io;

use super::messages;
use super::source::Sources;

pub struct WorkerActor;

impl Actor for WorkerActor {
  type Context = Context<Self>;

  fn started(&mut self, _ctx: &mut Self::Context) {
    info!("Starting WorkerActor");
  }
}

impl Handler<messages::RefreshSources> for WorkerActor {
  type Result = Result<Sources, io::Error>;

  fn handle(&mut self, msg: messages::RefreshSources, _: &mut Context<Self>) -> Self::Result {
    Ok(msg.sources)
  }
}
