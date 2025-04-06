//! task module: defines all types of task

pub mod base; // this only defines the trait
pub mod registry; // the main task registry

// specific task types
pub mod command_task;
pub mod lua_task;

// end.
