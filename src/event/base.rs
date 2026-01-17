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

use async_trait::async_trait;

use crate::common::logging::{LogType, log};
use crate::common::wres::{Error, Kind, Result};
use crate::condition::bucket_cond::ExecutionBucket;
use crate::condition::registry::ConditionRegistry;
use crate::constants::*;

#[async_trait(?Send)]
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

    /// Must return `false` if the event cannot be manually triggered:
    /// this is the default, and only manually triggerable events should
    /// override this function.
    fn triggerable(&self) -> bool {
        false
    }

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
    fn assign_condition(&mut self, cond_name: &str) -> Result<bool> {
        if let Some(cond_registry) = self.condition_registry() {
            if let Some(s) = cond_registry.condition_type(cond_name) {
                if s == "bucket" || s == "event" {
                    self._assign_condition(cond_name);
                    Ok(true)
                } else {
                    Err(Error::new(Kind::Unsupported, ERR_EVENT_INVALID_COND_TYPE))
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
    /// Also panics if the event has not been registered.
    fn fire_condition(&self) -> Result<bool> {
        assert!(
            self.get_id() != 0,
            "event {} not registered",
            self.get_name(),
        );
        assert!(self.get_condition().is_some(), "no condition assigned");
        assert!(
            self.condition_bucket().is_some(),
            "execution bucket not set",
        );

        let cond_name = self.get_condition().unwrap();
        let bucket = self.condition_bucket().unwrap();
        self.log(
            LogType::Debug,
            LOG_WHEN_PROC,
            LOG_STATUS_OK,
            &format!("condition {cond_name} firing"),
        );
        Ok(bucket.insert_condition(&cond_name)?)
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

    /// This function is a wrapper for the actual asynchronous event receiver:
    /// it returns the name of the event that triggered, mostly in order for
    /// the registry to issue a log line, or None for non triggered events
    async fn event_triggered(&mut self) -> Result<Option<String>> {
        self.fire_condition()?;
        Ok(Some(self.get_name()))
    }

    /// Setup for the listener initializing all internals if necessary: this
    /// must always be called before starting the loop that tests for the
    /// event to be fired. A successful initialization will return _true_,
    /// while a failing one (or no initialization at all) will return
    /// _false_. All erratic conditions should forward a suitable error.
    fn initial_setup(&mut self) -> Result<bool> {
        // the default implementation returns Ok(false) as it does nothing
        Ok(false)
    }

    /// Perform final cleanup if necessary. A successful cleanup will return
    /// _true_, while a failing one (or no initialization at all) will return
    /// _false_.
    fn final_cleanup(&mut self) -> Result<bool> {
        // the default implementation returns Ok(false) as it does nothing
        Ok(false)
    }

    /// Internal condition assignment function.
    fn _assign_condition(&mut self, cond_name: &str);
}

// define a type for boxed event references
pub type EventRef = Box<dyn Event>;

// end.
