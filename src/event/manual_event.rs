//! Define an empty event to be manually triggered
//!
//! The only way to trigger this type of event is to issue a `trigger`
//! command on the _stdin_ of a running instance of the scheduler, followed
//! by the event name. The event has no associated listening service, as the
//! main program acts as its service instead.

use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Arc;

use cfgmap::CfgMap;

use async_trait::async_trait;

use super::base::Event;
use crate::common::logging::{LogType, log};
use crate::common::wres::Result;
use crate::common::async_flip::AsyncFlip;
use crate::condition::bucket_cond::ExecutionBucket;
use crate::condition::registry::ConditionRegistry;
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
    triggered: Option<Arc<AsyncFlip>>,
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

// implement cloning
impl Clone for ManualCommandEvent {
    fn clone(&self) -> Self {
        ManualCommandEvent {
            // reset ID
            event_id: 0,

            // parameters
            event_name: self.event_name.clone(),
            condition_name: self.condition_name.clone(),

            // internal values
            condition_registry: None,
            condition_bucket: None,
            // specific members initialization
            // parameters

            // internal values
            triggered: None,
        }
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
            triggered: None,
        }
    }

    /// Load a `CommandLineEvent` from a [`CfgMap`](https://docs.rs/cfgmap/latest/)
    ///
    /// The `CommandLineEvent` is initialized according to the values
    /// provided in the `CfgMap` argument. If the `CfgMap` format does not
    /// comply with the requirements of a `FilesystemChangeEvent` an error is
    /// raised.
    pub fn load_cfgmap(
        cfgmap: &CfgMap,
        cond_registry: &'static ConditionRegistry,
        bucket: &'static ExecutionBucket,
    ) -> Result<ManualCommandEvent> {
        let check = vec!["type", "name", "tags", "condition"];
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
        let mut new_event = ManualCommandEvent::new(&name);
        new_event.condition_registry = Some(cond_registry);
        new_event.condition_bucket = Some(bucket);

        // common optional parameter initialization

        // tags are always simply checked this way as no value is needed
        let cur_key = "tags";
        if let Some(item) = cfgmap.get(cur_key)
            && !item.is_list()
            && !item.is_map()
        {
            return Err(cfg_err_invalid_config(
                cur_key,
                STR_UNKNOWN_VALUE,
                ERR_INVALID_PARAMETER,
            ));
        }

        let cur_key = "condition";
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

    /// Check a configuration map and return item name if Ok
    ///
    /// The check is performed exactly in the same way and in the same order
    /// as in `load_cfgmap`, the only difference is that no actual item is
    /// created and that a name is returned, which is the name of the item that
    /// _would_ be created via the equivalent call to `load_cfgmap`
    pub fn check_cfgmap(cfgmap: &CfgMap, available_conditions: &Vec<&str>) -> Result<String> {
        let check = vec!["type", "name", "tags", "condition"];
        cfg_check_keys(cfgmap, &check)?;

        // common mandatory parameter retrieval

        // type and name are both mandatory: type is checked and name is kept
        cfg_mandatory!(cfg_string_check_exact(cfgmap, "type", "cli"))?;
        let name = cfg_mandatory!(cfg_string_check_regex(cfgmap, "name", &RE_EVENT_NAME))?.unwrap();

        // also for optional parameters just check and throw away the result

        // tags are always simply checked this way
        let cur_key = "tags";
        if let Some(item) = cfgmap.get(cur_key)
            && !item.is_list()
            && !item.is_map()
        {
            return Err(cfg_err_invalid_config(
                cur_key,
                STR_UNKNOWN_VALUE,
                ERR_INVALID_PARAMETER,
            ));
        }

        // assigned condition is checked against the provided array
        let cur_key = "condition";
        if let Some(v) = cfg_string_check_regex(cfgmap, "condition", &RE_COND_NAME)?
            && !available_conditions.contains(&v.as_str())
        {
            return Err(cfg_err_invalid_config(
                cur_key,
                &v,
                ERR_INVALID_EVENT_CONDITION,
            ));
        }

        Ok(name)
    }
}

#[async_trait(?Send)]
impl Event for ManualCommandEvent {
    fn set_id(&mut self, id: i64) {
        self.event_id = id;
    }
    fn get_name(&self) -> String {
        self.event_name.clone()
    }
    fn get_id(&self) -> i64 {
        self.event_id
    }

    /// Return a hash of this item for comparison
    fn _hash(&self) -> u64 {
        let mut s = DefaultHasher::new();
        self.hash(&mut s);
        s.finish()
    }

    // this is the only event that overrides `triggerable()`
    fn triggerable(&self) -> bool {
        true
    }

    fn trigger(&self) -> bool {
        if let Some(triggered) = &self.triggered.clone() {
            triggered.flip();
            self.log(
                LogType::Debug,
                LOG_WHEN_PROC,
                LOG_STATUS_OK,
                "event has been manually triggered",
            );
            true
        } else {
            false
        }
    }

    fn get_condition(&self) -> Option<String> {
        self.condition_name.clone()
    }

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

    async fn event_triggered(&mut self) -> Result<Option<String>> {
        // create the future upon call to be sure that we are not polling
        // a completed future: it is discarded when completed, and recreated
        // at the subsequent call
        // WARNING: This code does not work!
        let triggered = Arc::new(AsyncFlip::new().await);
        self.triggered = Some(triggered.clone());
        if triggered.wait_flipped().await {
            self.log(
                LogType::Debug,
                LOG_WHEN_PROC,
                LOG_STATUS_OK,
                "manually triggered event caught",
            );
            self.triggered = None;
            match self.fire_condition() {
                Ok(res) => {
                    if res {
                        self.log(
                            LogType::Debug,
                            LOG_WHEN_PROC,
                            LOG_STATUS_OK,
                            "condition fired successfully",
                        );
                    } else {
                        self.log(
                            LogType::Trace,
                            LOG_WHEN_PROC,
                            LOG_STATUS_MSG,
                            "condition already fired: further schedule skipped",
                        );
                    }
                }
                Err(e) => {
                    self.log(
                        LogType::Warn,
                        LOG_WHEN_PROC,
                        LOG_STATUS_FAIL,
                        &format!("error firing condition: {e}"),
                    );
                }
            }
            Ok(Some(self.get_name()))
        } else {
            Ok(None)
        }
    }
}

// end.
