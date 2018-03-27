use actix::{msgs, Actor, Arbiter, Context};

pub struct CoordinatorActor;

impl Actor for CoordinatorActor {
  type Context = Context<Self>;

  fn started(&mut self, _ctx: &mut Self::Context) {
    info!("CoordinatorActor is alive!");
    Arbiter::system().do_send(msgs::SystemExit(0));
  }
}
