//! Define an empty event to be manually triggered
//!
//! The only way to trigger this type of event is to issue a `trigger`
//! command on the _stdin_ of a running instance of the scheduler, followed
//! by the event name. The event has no associated listening service, as the
//! main program acts as its service instead.


use cfgmap::CfgMap;

use super::base::Event;
use crate::condition::registry::ConditionRegistry;
use crate::condition::bucket_cond::ExecutionBucket;
use crate::common::logging::{log, LogType};
use crate::constants::*;



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


#[allow(dead_code)]
impl ManualCommandEvent {
    pub fn new(name: &str) -> Self {
        log(
            LogType::Debug,
            LOG_EMITTER_EVENT_FSCHANGE,
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
    /// type = "fschange"
    /// condition = "AssignedConditionName"
    ///
    /// Any incorrect value will cause an error. The value of the `type` entry
    /// *must* be set to `"fschange"` mandatorily for this type of `Event`.
    pub fn load_cfgmap(
        cfgmap: &CfgMap,
        cond_registry: &'static ConditionRegistry,
        bucket: &'static ExecutionBucket,
    ) -> std::io::Result<ManualCommandEvent> {

        fn _invalid_cfg(key: &str, value: &str, message: &str) -> std::io::Result<ManualCommandEvent> {
            Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("{ERR_INVALID_EVENT_CONFIG}: ({key}={value}) {message}"),
            ))
        }

        let check = [
            "type",
            "name",
            "tags",
            "condition",
        ];
        for key in cfgmap.keys() {
            if !check.contains(&key.as_str()) {
                return _invalid_cfg(key, STR_UNKNOWN_VALUE,
                    &format!("{ERR_INVALID_CFG_ENTRY} ({key})"));
            }
        }

        // check type
        let cur_key = "type";
        let cond_type;
        if let Some(item) = cfgmap.get(cur_key) {
            if !item.is_str() {
                return _invalid_cfg(
                    cur_key,
                    STR_UNKNOWN_VALUE,
                    ERR_INVALID_EVENT_TYPE);
            }
            cond_type = item.as_str().unwrap().to_owned();
            if cond_type != "cli" {
                return _invalid_cfg(cur_key,
                    &cond_type,
                    ERR_INVALID_EVENT_TYPE);
            }
        } else {
            return _invalid_cfg(
                cur_key,
                STR_UNKNOWN_VALUE,
                ERR_MISSING_PARAMETER);
        }

        // common mandatory parameter retrieval
        let cur_key = "name";
        let name;
        if let Some(item) = cfgmap.get(cur_key) {
            if !item.is_str() {
                return _invalid_cfg(
                    cur_key,
                    STR_UNKNOWN_VALUE,
                    ERR_INVALID_EVENT_NAME);
            }
            name = item.as_str().unwrap().to_owned();
            if !RE_EVENT_NAME.is_match(&name) {
                return _invalid_cfg(cur_key,
                    &name,
                    ERR_INVALID_EVENT_NAME);
            }
        } else {
            return _invalid_cfg(
                cur_key,
                STR_UNKNOWN_VALUE,
                ERR_MISSING_PARAMETER);
        }

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
        let cur_key = "tags";
        if let Some(item) = cfgmap.get(cur_key) {
            if !item.is_list() && !item.is_map() {
                return _invalid_cfg(
                    cur_key,
                    STR_UNKNOWN_VALUE,
                    ERR_INVALID_PARAMETER);
            }
        }

        let cur_key = "condition";
        let condition;
        if let Some(item) = cfgmap.get(cur_key) {
            if !item.is_str() {
                return _invalid_cfg(
                    cur_key,
                    STR_UNKNOWN_VALUE,
                    ERR_INVALID_EVENT_NAME);
            }
            condition = item.as_str().unwrap().to_owned();
            if !RE_COND_NAME.is_match(&condition) {
                return _invalid_cfg(cur_key,
                    &condition,
                    ERR_INVALID_COND_NAME);
            }
        } else {
            return _invalid_cfg(
                cur_key,
                STR_UNKNOWN_VALUE,
                ERR_MISSING_PARAMETER);
        }
        if !new_event.condition_registry.unwrap().has_condition(&condition) {
            return _invalid_cfg(
                cur_key,
                &condition,
                ERR_INVALID_EVENT_CONDITION);
        }
        new_event.assign_condition(&condition)?;

        // common optional parameter initialization
        // (none here)

        Ok(new_event)
    }

}


impl Event for ManualCommandEvent {

    fn set_id(&mut self, id: i64) { self.event_id = id; }
    fn get_name(&self) -> String { self.event_name.clone() }
    fn get_id(&self) -> i64 { self.event_id }

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


    fn _start_service(&self) -> std::io::Result<bool> {
        // in this case the service exits immediately without errors
        Ok(true)
    }

}


// end.
