//! Define event based on DBus subscriptions
//!
//! The user subscribes to a DBus event, specifying what to listen to and what
//! to expect from that channel. The event listens on a new thread and pushes
//! the related condition in the execution bucket when all constraints are met.

// this is only available when the "dbus" feature is enabled
#![cfg(feature = "dbus")]

use regex::Regex;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::{mpsc, Arc, RwLock};
use std::thread;

use futures::{channel::mpsc::channel, pin_mut, select, FutureExt, SinkExt, StreamExt};

use cfgmap::{CfgMap, CfgValue};

use async_std::task;
use zbus::{self, AsyncDrop, Message, MessageStream};

use serde_json::value::Value;
use std::str::FromStr;

use super::base::Event;
use crate::common::dbusitem::*;
use crate::common::logging::{log, LogType};
use crate::common::wres::{Error, Result, Kind};
use crate::condition::bucket_cond::ExecutionBucket;
use crate::condition::registry::ConditionRegistry;
use crate::{cfg_mandatory, constants::*};

use crate::cfghelp::*;

// see the DBus specification
const DBUS_MAX_NUMBER_OF_ARGUMENTS: i64 = 63;

/// DBus Based Event
///
/// Implements an event based upon DBus suscription to certain events, using
/// the [zbus](https://docs.rs/zbus/latest/zbus/) cross-platform pure Rust
/// DBus library. Configurations are provided for the different platforms.
///
/// **Note**: the `match_rule` holds a string implementing the *match rules*:
///           see [match rules](https://dbus.freedesktop.org/doc/dbus-specification.html#message-bus-routing-match-rules)
///           in the DBus specification for the exact (formal) syntax.
pub struct DbusMessageEvent {
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
    bus: Option<String>,
    match_rule: Option<String>,
    param_checks: Option<Vec<ParameterCheckTest>>,
    param_checks_all: bool,

    // internal values
    thread_running: RwLock<bool>,
    quit_tx: Option<mpsc::Sender<()>>,
}

// implement the hash protocol
impl Hash for DbusMessageEvent {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // common part
        self.event_name.hash(state);
        if let Some(s) = &self.condition_name {
            s.hash(state);
        }

        // specific part
        // 0 is hashed on the else branch in order to avoid that adjacent
        // strings one of which is undefined allow for hash collisions
        if let Some(x) = &self.bus {
            x.hash(state);
        } else {
            0.hash(state);
        }
        if let Some(x) = &self.match_rule {
            x.hash(state);
        } else {
            0.hash(state);
        }
        if let Some(x) = &self.param_checks {
            x.hash(state);
        } else {
            0.hash(state);
        }
        self.param_checks_all.hash(state);
    }
}

// implement cloning
impl Clone for DbusMessageEvent {
    fn clone(&self) -> Self {
        DbusMessageEvent {
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
            bus: self.bus.clone(),
            match_rule: self.match_rule.clone(),
            param_checks: {
                if let Some(o) = &self.param_checks {
                    let mut v: Vec<ParameterCheckTest> = Vec::new();
                    for t in o {
                        v.push(t.clone());
                    }
                    Some(v)
                } else {
                    None
                }
            },
            param_checks_all: self.param_checks_all,

            // internal values
            thread_running: RwLock::new(false),
            quit_tx: None,
        }
    }
}

#[allow(dead_code)]
impl DbusMessageEvent {
    /// Create a new `DbusMessageEvent` with the provided name
    pub fn new(name: &str) -> Self {
        log(
            LogType::Debug,
            LOG_EMITTER_EVENT_DBUS,
            LOG_ACTION_NEW,
            Some((name, 0)),
            LOG_WHEN_INIT,
            LOG_STATUS_MSG,
            &format!("EVENT {name}: creating a new DBus signal based event"),
        );
        DbusMessageEvent {
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
            bus: None,
            match_rule: None,
            param_checks: None,
            param_checks_all: false,

            // internal values
            thread_running: RwLock::new(false),
            quit_tx: None,
        }
    }

    /// Set the bus name to the provided value (checks for validity)
    pub fn set_bus(&mut self, name: &str) -> bool {
        if RE_DBUS_BUS_NAME.is_match(name) {
            self.bus = Some(String::from(name));
            return true;
        }
        false
    }

    /// Return an owned copy of the bus name
    pub fn bus(&self) -> Option<String> {
        self.bus.clone()
    }

    /// Set the match rule to the provided value (will check upon installation)
    pub fn set_match_rule(&mut self, rule: &str) -> bool {
        self.match_rule = Some(String::from(rule));
        true
    }

    /// Return an owned copy of the signal name
    pub fn match_rule(&self) -> Option<String> {
        self.match_rule.clone()
    }

    /// Load a `DbusMessageEvent` from a [`CfgMap`](https://docs.rs/cfgmap/latest/)
    ///
    /// The `DbusMessageEvent` is initialized according to the values
    /// provided in the `CfgMap` argument. If the `CfgMap` format does not
    /// comply with the requirements of a `DbusMessageEvent` an error is
    /// raised.
    ///
    /// Note that the values for the `parameter_check` entry are provided as
    /// JSON strings, because TOML is intentionally limited to accepting only
    /// lists of elements of the same type, and in our case we need to mix
    /// types both as arguments to a call and as index sequences.
    pub fn load_cfgmap(
        cfgmap: &CfgMap,
        cond_registry: &'static ConditionRegistry,
        bucket: &'static ExecutionBucket,
    ) -> Result<DbusMessageEvent> {
        fn _check_dbus_param_index(index: &CfgValue) -> Option<ParameterIndex> {
            if index.is_int() {
                let i = *index.as_int().unwrap();
                // as per specification, DBus supports at most 64 parameters
                if !(0..=DBUS_MAX_NUMBER_OF_ARGUMENTS).contains(&i) {
                    return None;
                } else {
                    return Some(ParameterIndex::Integer(i as u64));
                }
            } else if index.is_str() {
                let s = String::from(index.as_str().unwrap());
                return Some(ParameterIndex::String(s));
            }
            None
        }

        let check = vec![
            "type",
            "name",
            "tags",
            "condition",
            "bus",
            "rule",
            "parameter_check",
            "parameter_check_all",
        ];
        cfg_check_keys(cfgmap, &check)?;

        // common mandatory parameter retrieval

        // type and name are both mandatory but type is only checked
        cfg_mandatory!(cfg_string_check_exact(cfgmap, "type", "dbus"))?;
        let name = cfg_mandatory!(cfg_string_check_regex(cfgmap, "name", &RE_EVENT_NAME))?.unwrap();

        // specific mandatory parameter initialization
        let bus =
            cfg_mandatory!(cfg_string_check_regex(cfgmap, "bus", &RE_DBUS_MSGBUS_NAME))?.unwrap();
        let rule = cfg_mandatory!(cfg_string(cfgmap, "rule"))?.unwrap();

        // initialize the structure
        // NOTE: the value of "event" for the condition type, which is
        //       completely functionally equivalent to "bucket", can only
        //       be set from the configuration file; programmatically built
        //       conditions of this type will only report "bucket" as their
        //       type, and "event" is only left for configuration readability
        let mut new_event = DbusMessageEvent::new(&name);
        new_event.condition_registry = Some(cond_registry);
        new_event.condition_bucket = Some(bucket);
        new_event.bus = Some(bus);
        new_event.match_rule = Some(rule);

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

        // this is tricky: we build a list of elements constituted by:
        // - an index list (integers and strings, mixed) which will address
        //   every nested structure,
        // - an operator,
        // - a value to check against using the operator;
        // of course the value types found in TOML are less structured than the
        // ones supported by DBus, and subsequent tests will take this into
        // account and compare only values compatible with each other, and
        // compatible with the operator used
        let check = ["index", "operator", "value"];

        let cur_key = "parameter_check";
        if let Some(item) = cfgmap.get(cur_key) {
            let mut param_checks: Vec<ParameterCheckTest> = Vec::new();
            // here we expect a JSON string, reason explained above
            if !item.is_str() {
                return Err(cfg_err_invalid_config(
                    cur_key,
                    STR_UNKNOWN_VALUE,
                    ERR_INVALID_VALUE_FOR_ENTRY,
                ));
            }
            // since CfgMap only accepts maps as input, and we expect a list
            // instead, we build a map with a single element labeled '0':
            let json = Value::from_str(&format!("{{\"0\": {}}}", item.as_str().unwrap()));
            if json.is_err() {
                return Err(cfg_err_invalid_config(
                    cur_key,
                    STR_UNKNOWN_VALUE,
                    ERR_INVALID_VALUE_FOR_ENTRY,
                ));
            }
            // and then we extract the '0' element and check it to be a list
            let item = CfgMap::from_json(json.unwrap());
            let item = item.get("0").unwrap();
            if !item.is_list() {
                return Err(cfg_err_invalid_config(
                    cur_key,
                    STR_UNKNOWN_VALUE,
                    ERR_INVALID_VALUE_FOR_ENTRY,
                ));
            }
            let item = item.as_list().unwrap();
            for spec in item.iter() {
                if !spec.is_map() {
                    return Err(cfg_err_invalid_config(
                        cur_key,
                        STR_UNKNOWN_VALUE,
                        ERR_INVALID_VALUE_FOR_ENTRY,
                    ));
                }
                let spec = spec.as_map().unwrap();
                for key in spec.keys() {
                    if !check.contains(&key.as_str()) {
                        return Err(cfg_err_invalid_config(
                            &format!("{cur_key}:{key}"),
                            STR_UNKNOWN_VALUE,
                            &format!("{ERR_INVALID_CFG_ENTRY} ({key})"),
                        ));
                    }
                }
                let mut index_list: Vec<ParameterIndex> = Vec::new();
                if let Some(index) = spec.get("index") {
                    if index.is_int() {
                        if let Some(px) = _check_dbus_param_index(index) {
                            index_list.push(px);
                        } else {
                            return Err(cfg_err_invalid_config(
                                &format!("{cur_key}:index"),
                                &format!("{index:?}"),
                                ERR_INVALID_VALUE_FOR_ENTRY,
                            ));
                        }
                    } else if index.is_list() {
                        for sub_index in index.as_list().unwrap() {
                            if let Some(px) = _check_dbus_param_index(sub_index) {
                                index_list.push(px);
                            } else {
                                return Err(cfg_err_invalid_config(
                                    &format!("{cur_key}:index"),
                                    &format!("{sub_index:?}"),
                                    ERR_INVALID_VALUE_FOR_ENTRY,
                                ));
                            }
                        }
                    } else {
                        return Err(cfg_err_invalid_config(
                            &format!("{cur_key}:index"),
                            &format!("{index:?}"),
                            ERR_INVALID_VALUE_FOR_ENTRY,
                        ));
                    }
                } else {
                    return Err(cfg_err_invalid_config(
                        &format!("{cur_key}:index"),
                        STR_UNKNOWN_VALUE,
                        ERR_MISSING_PARAMETER,
                    ));
                }

                let operator;
                if let Some(oper) = spec.get("operator") {
                    if oper.is_str() {
                        operator = match oper.as_str().unwrap().as_str() {
                            "eq" => ParamCheckOperator::Equal,
                            "neq" => ParamCheckOperator::NotEqual,
                            "gt" => ParamCheckOperator::Greater,
                            "ge" => ParamCheckOperator::GreaterEqual,
                            "lt" => ParamCheckOperator::Less,
                            "le" => ParamCheckOperator::LessEqual,
                            "match" => ParamCheckOperator::Match,
                            "contains" => ParamCheckOperator::Contains,
                            "ncontains" => ParamCheckOperator::NotContains,
                            _ => {
                                return Err(cfg_err_invalid_config(
                                    &format!("{cur_key}:operator"),
                                    &format!("{oper:?}"),
                                    ERR_INVALID_VALUE_FOR_ENTRY,
                                ));
                            }
                        };
                    } else {
                        return Err(cfg_err_invalid_config(
                            &format!("{cur_key}:operator"),
                            STR_UNKNOWN_VALUE,
                            ERR_INVALID_VALUE_FOR_ENTRY,
                        ));
                    }
                } else {
                    return Err(cfg_err_invalid_config(
                        &format!("{cur_key}:operator"),
                        STR_UNKNOWN_VALUE,
                        ERR_MISSING_PARAMETER,
                    ));
                }

                let value;
                if let Some(v) = spec.get("value") {
                    if v.is_bool() {
                        value = ParameterCheckValue::Boolean(*v.as_bool().unwrap());
                    } else if v.is_int() {
                        value = ParameterCheckValue::Integer(*v.as_int().unwrap());
                    } else if v.is_float() {
                        value = ParameterCheckValue::Float(*v.as_float().unwrap());
                    } else if v.is_str() {
                        let s = v.as_str().unwrap();
                        if operator == ParamCheckOperator::Match {
                            let re = Regex::from_str(s);
                            if let Ok(re) = re {
                                value = ParameterCheckValue::Regex(re);
                            } else {
                                return Err(cfg_err_invalid_config(
                                    &format!("{cur_key}:value"),
                                    &format!("{v:?}"),
                                    ERR_INVALID_VALUE_FOR_ENTRY,
                                ));
                            }
                        } else {
                            value = ParameterCheckValue::String(s.to_string());
                        }
                    } else {
                        return Err(cfg_err_invalid_config(
                            &format!("{cur_key}:value"),
                            STR_UNKNOWN_VALUE,
                            ERR_INVALID_VALUE_FOR_ENTRY,
                        ));
                    }
                } else {
                    return Err(cfg_err_invalid_config(
                        &format!("{cur_key}:value"),
                        STR_UNKNOWN_VALUE,
                        ERR_MISSING_PARAMETER,
                    ));
                }
                // now that we have the full triple, we can add it to criteria
                param_checks.push(ParameterCheckTest {
                    index: index_list,
                    operator,
                    value,
                });
            }
            // finally the parameter checks become `Some` and makes its way
            // into the new event structure: the list is formally correct, but
            // it may not be compatible with the signal parameters, in which
            // case the parameter check will evaluate to _non-verified_ and a
            // warning log message will be issued (see below)
            new_event.param_checks = Some(param_checks);

            // `parameter_check_all` only makes sense if the paramenter check
            // list was built: for this reason it is set only in this case
            if let Some(v) = cfg_bool(cfgmap, "parameter_check_all")? {
                new_event.param_checks_all = v;
            }
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
        fn _check_dbus_param_index(index: &CfgValue) -> Option<ParameterIndex> {
            if index.is_int() {
                let i = *index.as_int().unwrap();
                // as per specification, DBus supports at most 64 parameters
                if !(0..=DBUS_MAX_NUMBER_OF_ARGUMENTS).contains(&i) {
                    return None;
                } else {
                    return Some(ParameterIndex::Integer(i as u64));
                }
            } else if index.is_str() {
                let s = String::from(index.as_str().unwrap());
                return Some(ParameterIndex::String(s));
            }
            None
        }

        let check = vec![
            "type",
            "name",
            "tags",
            "condition",
            "bus",
            "rule",
            "parameter_check",
            "parameter_check_all",
        ];
        cfg_check_keys(cfgmap, &check)?;

        // common mandatory parameter retrieval

        // type and name are both mandatory: type is checked and name is kept
        cfg_mandatory!(cfg_string_check_exact(cfgmap, "type", "dbus"))?;
        let name = cfg_mandatory!(cfg_string_check_regex(cfgmap, "name", &RE_EVENT_NAME))?.unwrap();

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

        let check = ["index", "operator", "value"];

        // see above for the reason why the check/configuration step is
        // performed like this: of course here no structure is created
        let cur_key = "parameter_check";
        if let Some(item) = cfgmap.get(cur_key) {
            // here we expect a JSON string, reason explained above
            if !item.is_str() {
                return Err(cfg_err_invalid_config(
                    cur_key,
                    STR_UNKNOWN_VALUE,
                    ERR_INVALID_VALUE_FOR_ENTRY,
                ));
            }
            // since CfgMap only accepts maps as input, and we expect a list
            // instead, we build a map with a single element labeled '0':
            let json = Value::from_str(&format!("{{\"0\": {}}}", item.as_str().unwrap()));
            if json.is_err() {
                return Err(cfg_err_invalid_config(
                    cur_key,
                    STR_UNKNOWN_VALUE,
                    ERR_INVALID_VALUE_FOR_ENTRY,
                ));
            }
            // and then we extract the '0' element and check it to be a list
            let item = CfgMap::from_json(json.unwrap());
            let item = item.get("0").unwrap();
            if !item.is_list() {
                return Err(cfg_err_invalid_config(
                    cur_key,
                    STR_UNKNOWN_VALUE,
                    ERR_INVALID_VALUE_FOR_ENTRY,
                ));
            }
            let item = item.as_list().unwrap();
            for spec in item.iter() {
                if !spec.is_map() {
                    return Err(cfg_err_invalid_config(
                        cur_key,
                        STR_UNKNOWN_VALUE,
                        ERR_INVALID_VALUE_FOR_ENTRY,
                    ));
                }
                let spec = spec.as_map().unwrap();
                for key in spec.keys() {
                    if !check.contains(&key.as_str()) {
                        return Err(cfg_err_invalid_config(
                            &format!("{cur_key}:{key}"),
                            STR_UNKNOWN_VALUE,
                            &format!("{ERR_INVALID_CFG_ENTRY} ({key})"),
                        ));
                    }
                }
                if let Some(index) = spec.get("index") {
                    if index.is_int() {
                        if _check_dbus_param_index(index).is_none() {
                            return Err(cfg_err_invalid_config(
                                &format!("{cur_key}:index"),
                                &format!("{index:?}"),
                                ERR_INVALID_VALUE_FOR_ENTRY,
                            ));
                        }
                    } else if index.is_list() {
                        for sub_index in index.as_list().unwrap() {
                            if _check_dbus_param_index(sub_index).is_none() {
                                return Err(cfg_err_invalid_config(
                                    &format!("{cur_key}:index"),
                                    &format!("{sub_index:?}"),
                                    ERR_INVALID_VALUE_FOR_ENTRY,
                                ));
                            }
                        }
                    } else {
                        return Err(cfg_err_invalid_config(
                            &format!("{cur_key}:index"),
                            &format!("{index:?}"),
                            ERR_INVALID_VALUE_FOR_ENTRY,
                        ));
                    }
                } else {
                    return Err(cfg_err_invalid_config(
                        &format!("{cur_key}:index"),
                        STR_UNKNOWN_VALUE,
                        ERR_MISSING_PARAMETER,
                    ));
                }

                // we keep the same method of checking the operator as above
                // instead of simply checking that the corresponding string
                // is present in a fixed array in order to check for regex
                // correctness below in the same way as load_cfgmap does
                let operator;
                if let Some(oper) = spec.get("operator") {
                    if oper.is_str() {
                        operator = match oper.as_str().unwrap().as_str() {
                            "eq" => ParamCheckOperator::Equal,
                            "neq" => ParamCheckOperator::NotEqual,
                            "gt" => ParamCheckOperator::Greater,
                            "ge" => ParamCheckOperator::GreaterEqual,
                            "lt" => ParamCheckOperator::Less,
                            "le" => ParamCheckOperator::LessEqual,
                            "match" => ParamCheckOperator::Match,
                            "contains" => ParamCheckOperator::Contains,
                            "ncontains" => ParamCheckOperator::NotContains,
                            _ => {
                                return Err(cfg_err_invalid_config(
                                    &format!("{cur_key}:operator"),
                                    &format!("{oper:?}"),
                                    ERR_INVALID_VALUE_FOR_ENTRY,
                                ));
                            }
                        };
                    } else {
                        return Err(cfg_err_invalid_config(
                            &format!("{cur_key}:operator"),
                            STR_UNKNOWN_VALUE,
                            ERR_INVALID_VALUE_FOR_ENTRY,
                        ));
                    }
                } else {
                    return Err(cfg_err_invalid_config(
                        &format!("{cur_key}:operator"),
                        STR_UNKNOWN_VALUE,
                        ERR_MISSING_PARAMETER,
                    ));
                }

                if let Some(v) = spec.get("value") {
                    if v.is_str() {
                        let s = v.as_str().unwrap();
                        if operator == ParamCheckOperator::Match {
                            let re = Regex::from_str(s);
                            if re.is_err() {
                                return Err(cfg_err_invalid_config(
                                    &format!("{cur_key}:value"),
                                    &format!("{v:?}"),
                                    ERR_INVALID_VALUE_FOR_ENTRY,
                                ));
                            }
                        }
                    } else if !(v.is_bool() || v.is_int() || v.is_float()) {
                        return Err(cfg_err_invalid_config(
                            &format!("{cur_key}:value"),
                            STR_UNKNOWN_VALUE,
                            ERR_INVALID_VALUE_FOR_ENTRY,
                        ));
                    }
                } else {
                    return Err(cfg_err_invalid_config(
                        &format!("{cur_key}:value"),
                        STR_UNKNOWN_VALUE,
                        ERR_MISSING_PARAMETER,
                    ));
                }
            }

            // `parameter_check_all` only makes sense if the parameter check
            // list was built: for this reason it is checked only in this case
            // (so that the checks are the same as the ones in load_cfgmap)
            cfg_bool(cfgmap, "parameter_check_all")?;
        }

        Ok(name)
    }
}

impl Event for DbusMessageEvent {
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

    fn requires_thread(&self) -> bool {
        true
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

    fn assign_quit_sender(&mut self, sr: mpsc::Sender<()>) {
        assert!(
            self.get_id() != 0,
            "event {} not registered",
            self.get_name()
        );
        self.quit_tx = Some(sr);
    }

    fn run_service(&self, qrx: Option<mpsc::Receiver<()>>) -> Result<bool> {
        assert!(
            qrx.is_some(),
            "quit signal channel receiver must be provided"
        );
        assert!(
            self.quit_tx.is_some(),
            "quit signal channel transmitter not initialized"
        );

        // unified event type that will be sent over an async channel by
        // either a `quit` command or the watcher: the `Target` option
        // contains the event generated by the watcher
        enum TargetOrQuitEvent {
            Target(Arc<Message>),
            Quit,
            QuitError,
        }

        // NOTE: the following helpers are async here, but since this service
        //       runs in a dedicated thread, we will just block on every step;
        //       the zbus::blocking option might be considered for further
        //       developement as well as rebuilding this service as real async

        // panic here if the bus name is incorrect: should have been fixed
        // when the event was configured and constructed
        async fn _get_connection(bus: &str) -> zbus::Result<zbus::Connection> {
            let connection;
            if bus == ":session" {
                connection = zbus::Connection::session().await;
            } else if bus == ":system" {
                connection = zbus::Connection::system().await;
            } else {
                panic!("specified bus `{bus}` not supported for event");
            }
            connection
        }

        // provide the message stream we subscribed to through the filter; note
        // that the rule is moved here since this will be the only consumer
        async fn _get_stream(
            rule: zbus::MatchRule<'_>,
            conn: zbus::Connection,
        ) -> zbus::Result<zbus::MessageStream> {
            zbus::MessageStream::for_match_rule(rule, &conn, None).await
        }

        // the following wraps a DBus message in a TargetOrQuitEvent in order
        // for the main loop to be allowed to quit and pushes it to a channel
        async fn _get_dbus_message(stream: &mut MessageStream) -> Option<TargetOrQuitEvent> {
            if let Some(m) = stream.next().await {
                if let Ok(m) = m {
                    Some(TargetOrQuitEvent::Target(m))
                } else {
                    None
                }
            } else {
                None
            }
        }

        // this function is built only for symmetry, in order to make clear
        // what is selected in the `select!` block within the async loop
        async fn _get_quit_message(
            rx: &mut futures::channel::mpsc::Receiver<TargetOrQuitEvent>,
        ) -> Option<TargetOrQuitEvent> {
            rx.next().await
        }

        // first unwrap all data needed to install the listening service:
        // this panics if any of the data is uninitialized because all the
        // mandatory parameters must be set, and any missing value would
        // indicate that there is a mistake in the program flow
        let bus = self
            .bus
            .clone()
            .expect("attempt to start service with uninitialized bus");

        let rule_str = self
            .match_rule
            .clone()
            .expect("attempt to start service with uninitialized match rule");

        let rule = zbus::MatchRule::try_from(rule_str.as_str())?;

        let conn = task::block_on(async {
            self.log(
                LogType::Debug,
                LOG_WHEN_START,
                LOG_STATUS_MSG,
                &format!("opening connection to bus `{bus}`"),
            );
            _get_connection(&bus).await
        })?;

        let dbus_stream = task::block_on(async {
            self.log(
                LogType::Debug,
                LOG_WHEN_START,
                LOG_STATUS_MSG,
                &format!("opening message stream on bus `{bus}`"),
            );
            _get_stream(rule, conn).await
        });
        if dbus_stream.is_err() {
            self.log(
                LogType::Warn,
                LOG_WHEN_START,
                LOG_STATUS_FAIL,
                &format!("could not subscribe to message on bus `{bus}`"),
            );
            return Ok(false);
        }
        let dbus_stream = dbus_stream.unwrap();

        self.log(
            LogType::Debug,
            LOG_WHEN_START,
            LOG_STATUS_OK,
            &format!("successfully subscribed to message on bus `{bus}`"),
        );

        // build an async communication channel for the quit signal
        let (aquit_tx, mut aquit_rx) = channel(EVENT_QUIT_CHANNEL_SIZE);

        // now it is time to set the internal `running` flag, before the
        // thread that waits for the quit signal is launched
        let mut running = self.thread_running.write().unwrap();
        *running = true;
        drop(running);

        // spawn a thread that only listens to a possible request to quit:
        // this thread should be lightweight enough, as it just waits all
        // the time; it is also useless to join to because it dies as soon
        // as it catches a signal
        let mut aq_tx_clone = aquit_tx.clone();
        let _quit_handle = thread::spawn(move || {
            match qrx.unwrap().recv() {
                Ok(_) => {
                    // send a quit message over the async channel
                    task::block_on({
                        async move {
                            aq_tx_clone.send(TargetOrQuitEvent::Quit).await.unwrap();
                        }
                    });
                }
                _ => {
                    // in case of error, send just the error option of the enum
                    task::block_on({
                        async move {
                            aq_tx_clone
                                .send(TargetOrQuitEvent::QuitError)
                                .await
                                .unwrap();
                        }
                    });
                }
            };
        });

        // clone the bus name
        let bus_name = bus.clone();

        // use a clone of the DBUS stream so that the original one can
        // be dropped below
        let mut dbus_stream_clone = dbus_stream.clone();

        // this should run in the local pool
        futures::executor::block_on(async move {
            'outer: loop {
                // wait on either one of the two possible messages
                let fdbus = _get_dbus_message(&mut dbus_stream_clone).fuse();
                let fquit = _get_quit_message(&mut aquit_rx).fuse();
                pin_mut!(fdbus, fquit);
                let nextmessage = select! {
                    md = fdbus => md,
                    mq = fquit => mq,
                };

                // first resolve the message into something that can be checked
                // or, alternatively, break out if the message instructs to quit;
                // actually, `msg` should never remain `None`
                let mut msg = None;
                if let Some(toq) = nextmessage {
                    match toq {
                        TargetOrQuitEvent::Target(m) => {
                            msg = Some(m);
                        }
                        TargetOrQuitEvent::QuitError => {
                            self.log(
                                LogType::Warn,
                                LOG_WHEN_PROC,
                                LOG_STATUS_FAIL,
                                "request to quit generated an error: exiting anyway",
                            );
                            break 'outer;
                        }
                        TargetOrQuitEvent::Quit => {
                            self.log(
                                LogType::Debug,
                                LOG_WHEN_END,
                                LOG_STATUS_OK,
                                "event listener termination request caught",
                            );
                            break 'outer;
                        }
                    }
                }

                // if we reached this point, the message has to be interpreted
                if let Some(message) = msg {
                    self.log(
                        LogType::Debug,
                        LOG_WHEN_PROC,
                        LOG_STATUS_OK,
                        &format!("subscribed message received on bus `{bus_name}`"),
                    );

                    // check message parameters against provided criteria
                    let verified;
                    if let Some(checks) = &self.param_checks {
                        self.log(
                            LogType::Debug,
                            LOG_WHEN_PROC,
                            LOG_STATUS_MSG,
                            &format!("parameter checks specified: {} checks must be verified", {
                                if self.param_checks_all {
                                    "all"
                                } else {
                                    "some"
                                }
                            },),
                        );

                        let severity;
                        let log_when;
                        let log_status;
                        let log_message;

                        (verified, severity, log_when, log_status, log_message) =
                            dbus_check_message(&message, checks, self.param_checks_all);
                        self.log(severity, log_when, log_status, &log_message);
                    } else {
                        // otherwise no parameters have been specified in
                        // the configuration file, check always positive
                        verified = true;
                    }

                    if verified {
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
                    } else {
                        self.log(
                            LogType::Debug,
                            LOG_WHEN_PROC,
                            LOG_STATUS_MSG,
                            "parameter check failed: condition not fired",
                        );
                    }
                } else {
                    // in normal conditions this is `unreachable!`
                    self.log(
                        LogType::Debug,
                        LOG_WHEN_PROC,
                        LOG_STATUS_MSG,
                        &format!("no messages on bus `{bus_name}`: exiting"),
                    );
                    // close the stream before shutting down
                    task::block_on(async { dbus_stream.async_drop().await });
                    break;
                }
            }
        }); // futures::executor::block_on(...)

        // as said above this should be ininfluent
        let _ = _quit_handle.join();

        self.log(
            LogType::Debug,
            LOG_WHEN_END,
            LOG_STATUS_OK,
            &format!("closing event listening service on bus `{bus}`"),
        );

        let mut running = self.thread_running.write().unwrap();
        *running = false;
        Ok(true)
    }

    fn stop_service(&self) -> Result<bool> {
        match self.thread_running.read() {
            Ok(running) => {
                if *running {
                    self.log(
                        LogType::Debug,
                        LOG_WHEN_END,
                        LOG_STATUS_OK,
                        "the listener has been requested to stop",
                    );
                    // send the quit signal
                    let quit_tx = self.quit_tx.clone();
                    if let Some(tx) = quit_tx {
                        tx.clone().send(()).unwrap();
                        Ok(true)
                    } else {
                        self.log(
                            LogType::Warn,
                            LOG_WHEN_END,
                            LOG_STATUS_OK,
                            "impossible to contact the listener: stop request dropped",
                        );
                        Ok(false)
                    }
                } else {
                    self.log(
                        LogType::Debug,
                        LOG_WHEN_END,
                        LOG_STATUS_OK,
                        "the service is not running: stop request dropped",
                    );
                    Ok(false)
                }
            }
            _ => {
                self.log(
                    LogType::Error,
                    LOG_WHEN_END,
                    LOG_STATUS_ERR,
                    "could not determine whether the service is running",
                );
                Err(Error::new(
                    Kind::Forbidden,
                    ERR_EVENT_LISTENING_NOT_DETERMINED,
                ))
            }
        }
    }

    fn thread_running(&self) -> Result<bool> {
        match self.thread_running.read() {
            Ok(running) => Ok(*running),
            _ => Err(Error::new(
                Kind::Forbidden,
                ERR_EVENT_LISTENING_NOT_DETERMINED,
            )),
        }
    }
}

// end.
