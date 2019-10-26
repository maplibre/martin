use actix::{Actor, Context, Handler};
use std::cell::RefCell;
use std::rc::Rc;

use crate::function_source::FunctionSources;
use crate::messages;
use crate::table_source::TableSources;

pub struct WorkerActor {
  pub table_sources: Rc<RefCell<Option<TableSources>>>,
  pub function_sources: Rc<RefCell<Option<FunctionSources>>>,
}

impl Actor for WorkerActor {
  type Context = Context<Self>;
}

impl Handler<messages::RefreshTableSources> for WorkerActor {
  type Result = ();

  fn handle(&mut self, msg: messages::RefreshTableSources, _: &mut Context<Self>) -> Self::Result {
    *self.table_sources.borrow_mut() = msg.table_sources.clone();
  }
}

impl Handler<messages::RefreshFunctionSources> for WorkerActor {
  type Result = ();

  fn handle(
    &mut self,
    msg: messages::RefreshFunctionSources,
    _: &mut Context<Self>,
  ) -> Self::Result {
    *self.function_sources.borrow_mut() = msg.function_sources.clone();
  }
}
