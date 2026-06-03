//! The [`Sink`] trait that reload drivers apply advisories against, and the
//! [`Trigger`] trait that decides when they reconcile.

mod sink;
mod trigger;

pub use sink::Sink;
pub use trigger::{NotifyTrigger, PollTrigger, Trigger};
