mod cfg_containers;

// Environment variable access with substitution tracking.
pub mod env;

pub use cfg_containers::*;

mod id_resolver;
pub use id_resolver::IdResolver;
