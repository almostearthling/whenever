//! Define a trait for events
//!
//! Events are represented here as structures that wait for something to
//! happen, and when that something happens that corresponds to a condition,
//! that condition is sent to the bucket of verified conditions and its related
//! tasks are executed at the subsequent tick. Waiting for an event can be
//! implemented in different ways depending on the event type: this includes
//!
//! * listening on a separate thread
//! * waiting for a signal or a message sent by the OS
//! * polling for any outcome
//!
//! and so on. The purpose of the trait is to provide a common interface that
//! is independent from the particular implementation of the waiting service.


// use std::sync::Arc;
// use std::sync::Mutex;
// use std::thread;
// use std::io::{Error, ErrorKind};

use crate::common::logging::{log, LogType};
use crate::condition::bucket_cond::ExecutionBucket;
use crate::condition::registry::ConditionRegistry;
use crate::constants::*;


#[allow(dead_code)]
pub trait Event: Send + Sync {

    /// Mandatory ID setter for registration.
    fn set_id(&mut self, id: i64);

    /// Return the name of the `Event` as an _owned_ `String`.
    fn get_name(&self) -> String;

    /// Return the ID of the `Event`.
    fn get_id(&self) -> i64;

    /// Retrieve the name of the assigned condition as an _owned_ `String`.
    fn get_condition(&self) -> Option<String>;

    /// Mandatory condition registry setter.
    fn set_condition_registry(&mut self, reg: &'static ConditionRegistry);

    /// Mandatory condition registry getter.
    fn condition_registry(&self) -> Option<&'static ConditionRegistry>;

    /// Must return `true` if the service requires a separate thread.
    fn requires_thread(&self) -> bool;

    /// Must return `false` if the event cannot be manually triggered:
    /// this is the default, and only manually triggerable events should
    /// override this function.
    fn triggerable(&self) -> bool { false }

    /// Assign the condition bucket to put verified conditions into.
    fn set_condition_bucket(&mut self, bucket: &'static ExecutionBucket);

    /// Condition bucket getter.
    fn condition_bucket(&self) -> Option<&'static ExecutionBucket>;


    /// Assign a condition to the event
    ///
    /// This function checks for the correct condition type, then uses the
    /// internal assignment method for actual attribution.
    ///
    /// # Panics
    ///
    /// When either the condition is not registered or the condition registry
    /// has not been set: each would indicate an error in the program flow.
    fn assign_condition(&mut self, cond_name: &str) -> std::io::Result<bool> {
        if let Some(cond_registry) = self.condition_registry() {
            if let Some(s) = cond_registry.condition_type(cond_name) {
                if s == "bucket" || s == "event" {
                    self._assign_condition(cond_name);
                    Ok(true)
                } else {
                    Err(std::io::Error::new(
                        std::io::ErrorKind::Unsupported,
                        ERR_EVENT_INVALID_COND_TYPE,
                    ))
                }
            } else {
                panic!("could not retrieve type of condition {cond_name}");
            }
        } else {
            panic!("condition registry not set");
        }
    }


    /// Fire the assigned condition by tossing it into the execution bucket
    ///
    /// This method must be invoked by the structures implementing the
    /// trait in order to fire the associated condition.
    ///
    /// # Panics
    ///
    /// When either the condition is not associated or the execution bucket
    /// has not been set: each would indicate an error in the program flow.
    /// Also panic if the event has not been registered.
    fn fire_condition(&self) -> std::io::Result<bool> {
        if self.get_id() == 0 {
            panic!("event {} not registered", self.get_name());
        }

        if let Some(cond_name) = self.get_condition() {
            if let Some(bucket) = self.condition_bucket() {
                self.log(
                    LogType::Info,
                    LOG_WHEN_PROC,
                    LOG_STATUS_OK,
                    &format!("condition {cond_name} firing"),
                );
                Ok(bucket.insert_condition(&cond_name))
            } else {
                panic!("execution bucket not set for condition {cond_name}")
            }
        } else {
            // self.log(
            //     LogType::Debug,
            //     LOG_WHEN_PROC,
            //     LOG_STATUS_FAIL,
            //     &format!("no condition associated to event"),
            // );
            // Ok(false)
            panic!("trying to fire with no condition assigned")
        }
    }


    /// Log a message in the specific `Event` format.
    ///
    /// This utility is provided so that all conditions can log in a consistent
    /// format, and has to be used for logging avoiding other kinds of output.
    /// The severity is provided as a parameter and must be one of the values
    /// listed in `common::LogType`.
    ///
    /// # Arguments
    ///
    /// * `severity` - one of `LogType::{Trace, Debug, Info, Warn, Error}`
    /// * `message` - the message to be logged as a borrowed string
    fn log(&self, severity: LogType, when: &str, status: &str, message: &str) {
        let name = self.get_name();
        let id = self.get_id();
        log(
            severity,
            LOG_EMITTER_EVENT,
            LOG_ACTION_ACTIVE,
            Some((&name, id)),
            when,
            status,
            message,
        );
    }


    /// The worker function for the event service: might require a separate
    /// thread (see above) or not; in the former case the service installer
    /// spawns the thread and returns a handle to join, and in the latter
    /// the installer just returns `None`. This method must return a boolean
    /// for possible outcome information, or fail with an error.
    ///
    /// **Note**: the worker function must be self-contained, in the sense that
    ///           it must _not_ modify the internals of the structure.
    fn _start_service(&self) -> std::io::Result<bool>;

    /// Internal condition assignment function.
    fn _assign_condition(&mut self, cond_name: &str);

}


// end.
