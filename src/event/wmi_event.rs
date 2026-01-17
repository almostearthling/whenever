//! Define a WMI query based event

#![cfg(windows)]
#![cfg(feature = "wmi")]

use std::hash::{DefaultHasher, Hash, Hasher};

use futures::StreamExt;

use cfgmap::CfgMap;

use async_trait::async_trait;

use wmi::WMIConnection;

use super::base::Event;
use crate::common::logging::{LogType, log};
use crate::common::wres::Result;
use crate::condition::bucket_cond::ExecutionBucket;
use crate::condition::registry::ConditionRegistry;
use crate::{cfg_mandatory, constants::*};

use crate::cfghelp::*;

/// WMI Based Event
///
/// Implements an event based upon WMI suscription to certain events, using
/// the [wmi](https://docs.rs/wmi/latest/wmi/) Windows-targeted WMI library.
///
/// **Note**: the `match_query` holds a string implementing the *WMI query*:
/// see https://learn.microsoft.com/en-us/windows/win32/wmisdk/receiving-a-wmi-event
pub struct WmiQueryEvent {
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
    namespace: Option<String>,
    match_query: Option<String>,
    // internal values
    // (none here)
}

impl Hash for WmiQueryEvent {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // common part
        self.event_name.hash(state);
        if let Some(s) = &self.condition_name {
            s.hash(state);
        }

        // specific part
        // 0 is hashed on the else branch in order to avoid that adjacent
        // strings one of which is undefined allow for hash collisions
        if let Some(x) = &self.match_query {
            x.hash(state);
        } else {
            0.hash(state);
        }

        if let Some(x) = &self.namespace {
            x.hash(state);
        } else {
            0.hash(state);
        }
    }
}

// implement cloning
impl Clone for WmiQueryEvent {
    fn clone(&self) -> Self {
        WmiQueryEvent {
            // reset ID
            event_id: 0,

            // parameters
            event_name: self.event_name.clone(),
            condition_name: self.condition_name.clone(),

            // internal values
            condition_registry: None,
            condition_bucket: None,

            // specific members
            // parameters
            namespace: self.namespace.clone(),
            match_query: self.match_query.clone(),
            // internal values
            // (none here)
        }
    }
}

#[allow(dead_code)]
impl WmiQueryEvent {
    /// Create a new `WmiQueryEvent` with the provided name
    pub fn new(name: &str) -> Self {
        log(
            LogType::Debug,
            LOG_EMITTER_EVENT_WMI,
            LOG_ACTION_NEW,
            Some((name, 0)),
            LOG_WHEN_INIT,
            LOG_STATUS_MSG,
            &format!("EVENT {name}: creating a new WMI query based event"),
        );
        WmiQueryEvent {
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
            namespace: None,
            match_query: None,
            // internal values
            // (none here)
        }
    }

    /// Set the match query to the provided value
    pub fn set_match_query(&mut self, query: &str) -> bool {
        self.match_query = Some(String::from(query));
        true
    }

    /// Return an owned copy of the match query
    pub fn match_query(&self) -> Option<String> {
        self.match_query.clone()
    }

    /// Set the namespace to the provided value
    pub fn set_namespace(&mut self, ns: &str) -> bool {
        self.namespace = Some(String::from(ns));
        true
    }

    /// Return an owned copy of the namespace
    pub fn namespace(&self) -> Option<String> {
        self.namespace.clone()
    }

    /// Load a `WmiQueryEvent` from a [`CfgMap`](https://docs.rs/cfgmap/latest/)
    ///
    /// The `WmiQueryEvent` is initialized according to the values
    /// provided in the `CfgMap` argument. If the `CfgMap` format does not
    /// comply with the requirements of a `WmiQueryEvent` an error is
    /// raised.
    pub fn load_cfgmap(
        cfgmap: &CfgMap,
        cond_registry: &'static ConditionRegistry,
        bucket: &'static ExecutionBucket,
    ) -> Result<WmiQueryEvent> {
        let check = vec!["type", "name", "tags", "condition", "namespace", "query"];
        cfg_check_keys(cfgmap, &check)?;

        // common mandatory parameter retrieval

        // type and name are both mandatory but type is only checked
        cfg_mandatory!(cfg_string_check_exact(cfgmap, "type", "wmi"))?;
        let name = cfg_mandatory!(cfg_string_check_regex(cfgmap, "name", &RE_EVENT_NAME))?.unwrap();

        // specific mandatory parameter initialization
        let query = cfg_mandatory!(cfg_string(cfgmap, "query"))?.unwrap();

        // initialize the structure
        // NOTE: the value of "event" for the condition type, which is
        //       completely functionally equivalent to "bucket", can only
        //       be set from the configuration file; programmatically built
        //       conditions of this type will only report "bucket" as their
        //       type, and "event" is only left for configuration readability
        let mut new_event = WmiQueryEvent::new(&name);
        new_event.condition_registry = Some(cond_registry);
        new_event.condition_bucket = Some(bucket);
        new_event.match_query = Some(query);

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

        let cur_key = "condition";
        if let Some(v) = cfg_string_check_regex(cfgmap, "condition", &RE_COND_NAME)? {
            if !new_event.condition_registry.unwrap().has_condition(&v)? {
                return Err(cfg_err_invalid_config(
                    cur_key,
                    &v,
                    ERR_INVALID_EVENT_CONDITION,
                ));
            }
            new_event.assign_condition(&v)?;
        }

        // specific optional parameter initialization
        if let Some(v) = cfg_string_check_regex(cfgmap, "namespace", &RE_WMI_NAMESPACE)? {
            new_event.namespace = Some(v.replace("/", "\\"));
        }

        Ok(new_event)
    }

    /// Check a configuration map and return item name if Ok
    ///
    /// The check is performed exactly in the same way and in the same order
    /// as in `load_cfgmap`, the only difference is that no actual item is
    /// created and that a name is returned, which is the name of the item that
    /// _would_ be created via the equivalent call to `load_cfgmap`
    pub fn check_cfgmap(cfgmap: &CfgMap, available_conditions: &Vec<&str>) -> Result<String> {
        let check = vec!["type", "name", "tags", "condition", "namespace", "query"];
        cfg_check_keys(cfgmap, &check)?;

        // common mandatory parameter retrieval

        // type and name are both mandatory: type is checked and name is kept
        cfg_mandatory!(cfg_string_check_exact(cfgmap, "type", "wmi"))?;
        let name = cfg_mandatory!(cfg_string_check_regex(cfgmap, "name", &RE_EVENT_NAME))?.unwrap();
        cfg_mandatory!(cfg_string(cfgmap, "query"))?;

        // also for optional parameters just check and throw away the result

        // tags are always simply checked this way
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

        // assigned condition is checked against the provided array
        let cur_key = "condition";
        if let Some(v) = cfg_string_check_regex(cfgmap, "condition", &RE_COND_NAME)? {
            if !available_conditions.contains(&v.as_str()) {
                return Err(cfg_err_invalid_config(
                    cur_key,
                    &v,
                    ERR_INVALID_EVENT_CONDITION,
                ));
            }
        }

        // specific optional parameter check
        cfg_string_check_regex(cfgmap, "namespace", &RE_WMI_NAMESPACE)?;

        Ok(name)
    }
}

#[async_trait(?Send)]
impl Event for WmiQueryEvent {
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
        let name = self.get_name();
        assert!(
            self.match_query.is_some(),
            "match_query not set for WmiQueryEvent {name}",
        );

        // the following are to enable the execution of a WMI async query
        let conn = if self.namespace.is_some() {
            let namespace = self.namespace.as_ref().unwrap();
            self.log(
                LogType::Trace,
                LOG_WHEN_PROC,
                LOG_STATUS_OK,
                &format!("opening WMI connection to namespace `{namespace}`"),
            );
            WMIConnection::with_namespace_path(namespace)?
        } else {
            self.log(
                LogType::Trace,
                LOG_WHEN_PROC,
                LOG_STATUS_OK,
                "opening WMI connection to default namespace",
            );
            WMIConnection::new()?
        };

        let query = self.match_query.clone().unwrap();
        let mut event_receiver = conn.exec_notification_query_async(query)?;
        self.log(
            LogType::Trace,
            LOG_WHEN_PROC,
            LOG_STATUS_OK,
            "successfully subscribed/resubscribed to WMI event",
        );

        while let Some(evt) = event_receiver.next().await {
            self.log(
                LogType::Debug,
                LOG_WHEN_PROC,
                LOG_STATUS_OK,
                "event received through WMI communication channel",
            );
            if let Err(e) = evt {
                self.log(
                    LogType::Debug,
                    LOG_WHEN_PROC,
                    LOG_STATUS_ERR,
                    &format!("WMI error received: {e}"),
                );
                return Ok(None);
            } else {
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
            }
        }

        Ok(Some(name))
    }

    // because of the nature of the WMI library, a connection and the related
    // event stream cannot be sent safely across threads, so this stateful
    // part will be created directly in the async event poller: it costs some
    // extra effort each time an event poller is reinstated; therefore the
    // preparation step is left as the default implementation
    // fn initial_setup(&mut self) -> Result<bool> {
    //     // the default implementation returns Ok(false) as it does nothing
    //     Ok(false)
    // }
}

// end.
