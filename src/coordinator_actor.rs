use actix::{Actor, Addr, Context, Handler};

use crate::messages;
use crate::worker_actor::WorkerActor;

#[derive(Default)]
pub struct CoordinatorActor {
    workers: Vec<Addr<WorkerActor>>,
}

impl Actor for CoordinatorActor {
    type Context = Context<Self>;
}

impl Handler<messages::Connect> for CoordinatorActor {
    type Result = Addr<WorkerActor>;

    fn handle(&mut self, msg: messages::Connect, _: &mut Context<Self>) -> Self::Result {
        self.workers.push(msg.addr.clone());
        msg.addr
    }
}

impl Handler<messages::RefreshTableSources> for CoordinatorActor {
    type Result = ();

    fn handle(
        &mut self,
        msg: messages::RefreshTableSources,
        _: &mut Context<Self>,
    ) -> Self::Result {
        for worker in &self.workers {
            let message = messages::RefreshTableSources {
                table_sources: msg.table_sources.clone(),
            };
            worker.do_send(message);
        }
    }
}

impl Handler<messages::RefreshFunctionSources> for CoordinatorActor {
    type Result = ();

    fn handle(
        &mut self,
        msg: messages::RefreshFunctionSources,
        _: &mut Context<Self>,
    ) -> Self::Result {
        for worker in &self.workers {
            let message = messages::RefreshFunctionSources {
                function_sources: msg.function_sources.clone(),
            };
            worker.do_send(message);
        }
    }
}
