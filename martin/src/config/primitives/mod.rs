mod auto_option;
pub use auto_option::AutoOption;
mod opt_bool_obj;
pub use opt_bool_obj::OptBoolObj;
mod id_resolver;
pub mod one_or_many;
pub use id_resolver::IdResolver;

// Environment variable access with substitution tracking.
pub mod env;
