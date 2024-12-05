//! Define an empty event to be manually triggered
//!
//! The only way to trigger this type of event is to issue a `trigger`
//! command on the _stdin_ of a running instance of the scheduler, followed
//! by the event name. The event has no associated listening service, as the
//! main program acts as its service instead.


use std::hash::{DefaultHasher, Hash, Hasher};

use cfgmap::CfgMap;

use super::base::Event;
use crate::condition::registry::ConditionRegistry;
use crate::condition::bucket_cond::ExecutionBucket;
use crate::common::logging::{log, LogType};
use crate::{cfg_mandatory, constants::*};

use crate::cfghelp::*;



/// Manual Command Based Event
///
/// Implements an event that is intentionally triggered by issuing a `trigger`
/// command, followed by the event name, to the _stdin_ of a running instance.
pub struct ManualCommandEvent {
    // common members
    // parameters
    event_id: i64,
    event_name: String,
    condition_name: Option<String>,

    // internal values
    condition_registry: Option<&'static ConditionRegistry>,
    condition_bucket: Option<&'static ExecutionBucket>,

    // specific members
    // parameters
    // (none here)

    // internal values
    // (none here)
}

// implement the hash protocol
impl Hash for ManualCommandEvent {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // common part
        self.event_name.hash(state);
        if let Some(s) = &self.condition_name {
            s.hash(state);
        }

        // specific part
        // (none here)
    }

}


#[allow(dead_code)]
impl ManualCommandEvent {
    pub fn new(name: &str) -> Self {
        log(
            LogType::Debug,
            LOG_EMITTER_EVENT_MANUAL,
            LOG_ACTION_NEW,
            Some((name, 0)),
            LOG_WHEN_INIT,
            LOG_STATUS_MSG,
            &format!("EVENT {name}: creating a new command line triggered event"),
        );
        ManualCommandEvent {
            // reset ID
            event_id: 0,

            // parameters
            event_name: String::from(name),
            condition_name: None,

            // internal values
            condition_registry: None,
            condition_bucket: None,

            // specific members initialization
            // parameters

            // internal values
        }
    }



    /// Load a `CommandLineEvent` from a [`CfgMap`](https://docs.rs/cfgmap/latest/)
    ///
    /// The `CommandLineEvent` is initialized according to the values
    /// provided in the `CfgMap` argument. If the `CfgMap` format does not
    /// comply with the requirements of a `FilesystemChangeEvent` an error is
    /// raised.
    ///
    /// The TOML configuration file format is the following
    ///
    /// ```toml
    /// # definition (mandatory)
    /// [[event]]
    /// name = "FilesystemChangeEventName"
    /// type = "cli"
    /// condition = "AssignedConditionName"
    /// ```
    ///
    /// Any incorrect value will cause an error. The value of the `type` entry
    /// *must* be set to `"cli"` mandatorily for this type of `Event`.
    pub fn load_cfgmap(
        cfgmap: &CfgMap,
        cond_registry: &'static ConditionRegistry,
        bucket: &'static ExecutionBucket,
    ) -> std::io::Result<ManualCommandEvent> {

        let check = vec![
            "type",
            "name",
            "tags",
            "condition",
        ];
        cfg_check_keys(cfgmap, &check)?;

        // common mandatory parameter retrieval

        // type and name are both mandatory but type is only checked
        cfg_mandatory!(cfg_string_check_exact(cfgmap, "type", "cli"))?;
        let name = cfg_mandatory!(cfg_string_check_regex(cfgmap, "name", &RE_EVENT_NAME))?.unwrap();

        // initialize the structure
        // NOTE: the value of "event" for the condition type, which is
        //       completely functionally equivalent to "bucket", can only
        //       be set from the configuration file; programmatically built
        //       conditions of this type will only report "bucket" as their
        //       type, and "event" is only left for configuration readability
        let mut new_event = ManualCommandEvent::new(
            &name,
        );
        new_event.condition_registry = Some(cond_registry);
        new_event.condition_bucket = Some(bucket);

        // common optional parameter initialization

        // tags are always simply checked this way as no value is needed
        let cur_key = "tags";
        if let Some(item) = cfgmap.get(cur_key) {
            if !item.is_list() && !item.is_map() {
                return Err(cfg_err_invalid_config(
                    cur_key,
                    STR_UNKNOWN_VALUE,
                    ERR_INVALID_PARAMETER,
                ));
            }
        }

        if let Some(v) = cfg_string_check_regex(cfgmap, "condition", &RE_COND_NAME)? {
            if !new_event.condition_registry.unwrap().has_condition(&v) {
                return Err(cfg_err_invalid_config(
                    cur_key,
                    &v,
                    ERR_INVALID_EVENT_CONDITION,
                ));
            }
            new_event.assign_condition(&v)?;
        }

        // specific optional parameter initialization
        // (none here)

        Ok(new_event)
    }

}


impl Event for ManualCommandEvent {

    fn set_id(&mut self, id: i64) { self.event_id = id; }
    fn get_name(&self) -> String { self.event_name.clone() }
    fn get_id(&self) -> i64 { self.event_id }

    /// Return a hash of this item for comparison
    fn _hash(&self) -> u64 {
        let mut s = DefaultHasher::new();
        self.hash(&mut s);
        s.finish()
    }


    fn requires_thread(&self) -> bool { false }

    fn triggerable(&self) -> bool { true }      // this is the only one

    fn get_condition(&self) -> Option<String> { self.condition_name.clone() }

    fn set_condition_registry(&mut self, reg: &'static ConditionRegistry) {
        self.condition_registry = Some(reg);
    }

    fn condition_registry(&self) -> Option<&'static ConditionRegistry> {
        self.condition_registry
    }

    fn set_condition_bucket(&mut self, bucket: &'static ExecutionBucket) {
        self.condition_bucket = Some(bucket);
    }

    fn condition_bucket(&self) -> Option<&'static ExecutionBucket> {
        self.condition_bucket
    }

    fn _assign_condition(&mut self, cond_name: &str) {
        // correctness has already been checked by the caller
        self.condition_name = Some(String::from(cond_name));
    }


    fn _run_service(&self) -> std::io::Result<bool> {
        // in this case the service exits immediately without errors
        Ok(true)
    }

    fn _stop_service(&self) -> std::io::Result<bool> {
        // in this case the service is already stopped
        Ok(true)
    }

    fn _thread_running(&self) -> std::io::Result<bool> {
        // no special thread is running for this kind of event
        Ok(false)
    }

}


// end.
