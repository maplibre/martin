use actix::prelude::*;
use std::rc::Rc;
use std::cell::RefCell;

use super::messages;
use super::source::Sources;

pub struct WorkerActor {
  pub sources: Rc<RefCell<Sources>>,
}

impl Actor for WorkerActor {
  type Context = Context<Self>;

  fn started(&mut self, _ctx: &mut Self::Context) {
    info!("Starting WorkerActor");
  }
}

impl Handler<messages::RefreshSources> for WorkerActor {
  type Result = ();

  fn handle(&mut self, msg: messages::RefreshSources, _: &mut Context<Self>) -> Self::Result {
    *self.sources.borrow_mut() = msg.sources.clone();
  }
}
