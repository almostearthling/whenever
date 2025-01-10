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


use std::sync::mpsc;

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

    /// Tell whether or not another `Event` is equal to this
    fn eq(&self, other: &dyn Event) -> bool {
        self._hash() == other._hash()
    }

    /// Tell whether or not another `Event` is not equal to this
    fn ne(&self, other: &dyn Event) -> bool {
        !self.eq(other)
    }

    /// Internally called to implement `eq()` and `neq()`: hash calculation
    /// is costly in terms of time and possibly CPU, but it is supposed to
    /// take place very seldomly, near to almost never
    fn _hash(&self) -> u64;


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

    /// Assign a quit signal sender before the service starts.
    ///
    /// The default implementation is only suitable for events that do not
    /// require a listener, all other event type must reimplement it.
    ///
    /// # Panics
    ///
    /// Panics if the event has not been registered.
    fn assign_quit_sender(&mut self, _sr: mpsc::Sender<()>) {
        assert!(self.get_id() != 0, "event {} not registered", self.get_name());
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
    /// Also panics if the event has not been registered.
    fn fire_condition(&self) -> std::io::Result<bool> {
        assert!(self.get_id() != 0, "event {} not registered", self.get_name());
        assert!(self.get_condition().is_some(), "no condition assigned");
        assert!(self.condition_bucket().is_some(), "execution bucket not set");

        let cond_name = self.get_condition().unwrap();
        let bucket = self.condition_bucket().unwrap();
        self.log(
            LogType::Info,
            LOG_WHEN_PROC,
            LOG_STATUS_OK,
            &format!("condition {cond_name} firing"),
        );
        Ok(bucket.insert_condition(&cond_name))
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
    /// for possible outcome information, or fail with an error. In case it
    /// requires a separate thread, the receiving end of a channel must be
    /// passed to the service, over which a unit value will be sent in order
    /// to let the service know it must stop: the running thread must obey
    /// receiving a `()` over the channel and leave immediately as soon as
    /// it happens.
    ///
    /// **Note**: the worker function must be self-contained, in the sense that
    ///           it must _not_ modify the internals of the structure, apart
    ///           from a flag that states that the service thread is running.
    ///
    /// The default implementation is only suitable for events that do not
    /// require a listener, all other event type must reimplement it.
    fn run_service(&self, qrx: Option<mpsc::Receiver<()>>) -> std::io::Result<bool> {
        // in this case the service exits immediately without errors
        assert!(qrx.is_none(), "quit signal channel provided for event without listener");
        Ok(true)
    }

    /// This must be called to stop the event listening service.
    ///
    /// The default implementation is only suitable for events that do not
    /// require a listener, all other event type must reimplement it.
    fn stop_service(&self) -> std::io::Result<bool> {
        Ok(true)
    }

    /// This tells whether the service thread (if any) is active or not.
    ///
    /// The default implementation is only suitable for events that do not
    /// require a listener, all other event type must reimplement it.
    fn thread_running(&self) -> std::io::Result<bool> {
        // no special thread is running for this kind of event
        Ok(false)
    }

    /// Internal condition assignment function.
    fn _assign_condition(&mut self, cond_name: &str);

}


// end.
