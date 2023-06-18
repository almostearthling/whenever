//! condition module: defines all types of condition

pub mod base;       // this only defines the trait
pub mod registry;   // the main condition registry

// specific condition types
pub mod interval_cond;
pub mod idle_cond;
pub mod time_cond;
pub mod command_cond;
pub mod lua_cond;
pub mod bucket_cond;
pub mod dbus_cond;

// end.
