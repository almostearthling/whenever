//! Define a DBus method invocation based condition
//!
//! This type of condition is verified whenever an invocation to a provided
//! DBus method return a value that meets the criteria specified in the
//! configuration. The difference with the event based process consists in
//! the condition actively requesting DBus for a result.

// this is only available when the "dbus" feature is enabled
#![cfg(feature = "dbus")]

use std::hash::{DefaultHasher, Hash, Hasher};
use std::time::{Duration, Instant};

use cfgmap::{CfgMap, CfgValue};
use regex::Regex;

use async_std::task;
use zbus;
use zbus::zvariant;

use std::str::FromStr;

use super::base::Condition;
use crate::common::dbusitem::*;
use crate::common::logging::{LogType, log};
use crate::common::wres::Result;
use crate::task::registry::TaskRegistry;
use crate::{cfg_mandatory, constants::*};

use crate::cfghelp::*;

// see the DBus specification
const DBUS_MAX_NUMBER_OF_ARGUMENTS: i64 = 63;

/// DBus Method Based Condition
///
/// This condition is verified whenever a value returned by a DBus method
/// invocation meets the criteria specified in the configuration.
pub struct DbusMethodCondition {
    // commom members
    // parameters
    cond_id: i64,
    cond_name: String,
    task_names: Vec<String>,
    recurring: bool,
    max_retries: i64,
    exec_sequence: bool,
    break_on_failure: bool,
    break_on_success: bool,
    suspended: bool,

    // internal values
    has_succeeded: bool,
    last_tested: Option<Instant>,
    last_succeeded: Option<Instant>,
    startup_time: Option<Instant>,
    task_registry: Option<&'static TaskRegistry>,
    left_retries: i64,
    tasks_failed: bool,

    // specific members
    // parameters
    bus: Option<String>,
    service: Option<String>,
    object_path: Option<String>,
    interface: Option<String>,
    method: Option<String>,
    param_call: Option<Vec<zvariant::OwnedValue>>,
    param_checks: Option<Vec<ParameterCheckTest>>,
    param_checks_all: bool,
    recur_after_failed_check: bool,
    check_after: Option<Duration>,

    // internal values
    check_last: Instant,

    // this is different from has_succeeded: the latter is set when the
    // condition has actually been successful, which in this case may not
    // be true, as a persistent success may not let the condition succeed
    last_check_failed: bool,
}

// implement the hash protocol
impl Hash for DbusMethodCondition {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // common part
        self.cond_name.hash(state);
        self.recurring.hash(state);
        self.max_retries.hash(state);
        self.exec_sequence.hash(state);
        self.break_on_failure.hash(state);
        self.break_on_success.hash(state);
        // suspended is more a status: let's not consider it yet
        // self.suspended.hash(state);
        // task order is significant: hash on vec is not sorted
        self.task_names.hash(state);

        // specific part
        self.param_checks_all.hash(state);

        // 0 is hashed on the else branch in order to avoid that adjacent
        // strings one of which is undefined allow for hash collisions
        if let Some(x) = &self.bus {
            x.hash(state);
        } else {
            0.hash(state);
        }
        if let Some(x) = &self.service {
            x.hash(state);
        } else {
            0.hash(state);
        }
        if let Some(x) = &self.object_path {
            x.hash(state);
        } else {
            0.hash(state);
        }
        if let Some(x) = &self.interface {
            x.hash(state);
        } else {
            0.hash(state);
        }
        if let Some(x) = &self.method {
            x.hash(state);
        } else {
            0.hash(state);
        }

        // let's hope that to_string is a correct representation of elem
        if let Some(x) = &self.param_call {
            for elem in x {
                elem.to_string().hash(state);
            }
        } else {
            0.hash(state);
        }

        self.param_checks.hash(state);
        self.param_checks_all.hash(state);
        self.check_after.hash(state);
        self.recur_after_failed_check.hash(state);
    }
}

#[allow(dead_code)]
impl DbusMethodCondition {
    /// Create a new DBus method invocation based condition with the given name
    pub fn new(name: &str) -> Self {
        log(
            LogType::Debug,
            LOG_EMITTER_CONDITION_DBUS,
            LOG_ACTION_NEW,
            Some((name, 0)),
            LOG_WHEN_INIT,
            LOG_STATUS_MSG,
            &format!("CONDITION {name}: creating a new DBus method based condition"),
        );
        let t = Instant::now();
        DbusMethodCondition {
            // common members initialization
            // reset ID
            cond_id: 0,

            // parameters
            cond_name: String::from(name),
            task_names: Vec::new(),
            recurring: false,
            max_retries: 0,
            exec_sequence: true,
            break_on_failure: false,
            break_on_success: false,
            suspended: true,

            // internal values
            startup_time: None,
            last_tested: None,
            last_succeeded: None,
            has_succeeded: false,
            task_registry: None,
            left_retries: 0,
            tasks_failed: false,

            // specific members initialization
            // parameters
            check_after: None,
            bus: None,
            service: None,
            object_path: None,
            interface: None,
            method: None,
            param_call: None,
            param_checks: None,
            param_checks_all: false,
            recur_after_failed_check: false,

            // internal values
            check_last: t,
            last_check_failed: true,
        }
    }

    // constructor modifiers
    /// Set the command execution to sequence or parallel
    pub fn execs_sequentially(mut self, yes: bool) -> Self {
        self.exec_sequence = yes;
        self
    }

    /// If true, *sequential* task execution will break on first success
    pub fn breaks_on_success(mut self, yes: bool) -> Self {
        self.break_on_success = yes;
        self
    }

    /// If true, *sequential* task execution will break on first failure
    pub fn breaks_on_failure(mut self, yes: bool) -> Self {
        self.break_on_failure = yes;
        self
    }

    /// If true, create a recurring condition
    pub fn repeats(mut self, yes: bool) -> Self {
        self.recurring = yes;
        self
    }

    /// Retry `num` times on task failure if not recurring
    pub fn retries(mut self, num: i64) -> Self {
        assert!(num >= -1, "max number of retries must be positive or -1");
        self.max_retries = num;
        self
    }

    /// Constructor modifier to specify that the condition is verified on
    /// check success only if there has been at least one failure after the
    /// last successful test
    pub fn recurs_after_check_failure(mut self, yes: bool) -> Self {
        self.recur_after_failed_check = yes;
        self
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

    /// Set the service name to the provided value (checks for validity)
    pub fn set_service(&mut self, name: &str) -> bool {
        if RE_DBUS_SERVICE_NAME.is_match(name) {
            self.service = Some(String::from(name));
            return true;
        }
        false
    }

    /// Return an owned copy of the service name
    pub fn service(&self) -> Option<String> {
        self.service.clone()
    }

    /// Set the object path to the provided value (checks for validity)
    pub fn set_object_path(&mut self, name: &str) -> bool {
        if RE_DBUS_OBJECT_PATH.is_match(name) {
            self.object_path = Some(String::from(name));
            return true;
        }
        false
    }

    /// Return an owned copy of the object path
    pub fn object_path(&self) -> Option<String> {
        self.object_path.clone()
    }

    /// Set the interface name to the provided value (checks for validity)
    pub fn set_interface(&mut self, name: &str) -> bool {
        if RE_DBUS_INTERFACE_NAME.is_match(name) {
            self.interface = Some(String::from(name));
            return true;
        }
        false
    }

    /// Return an owned copy of the interface name
    pub fn interface(&self) -> Option<String> {
        self.interface.clone()
    }

    /// Load a `DbusMethodCondition` from a [`CfgMap`](https://docs.rs/cfgmap/latest/)
    ///
    /// The `DbusMethodCondition` is initialized according to the values
    /// provided in the `CfgMap` argument. If the `CfgMap` format does not
    /// comply with the requirements of a `DbusMethodCondition` an error is
    /// raised.
    pub fn load_cfgmap(
        cfgmap: &CfgMap,
        task_registry: &'static TaskRegistry,
    ) -> Result<DbusMethodCondition> {
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
            "tasks",
            "recurring",
            "max_tasks_retries",
            "execute_sequence",
            "break_on_failure",
            "break_on_success",
            "suspended",
            "check_after",
            "bus",
            "service",
            "object_path",
            "interface",
            "method",
            "parameter_call",
            "parameter_check_all",
            "parameter_check",
            "recur_after_failed_check",
        ];
        cfg_check_keys(cfgmap, &check)?;

        // common mandatory parameter retrieval

        // type and name are both mandatory but type is only checked
        cfg_mandatory!(cfg_string_check_exact(cfgmap, "type", "dbus"))?;
        let name = cfg_mandatory!(cfg_string_check_regex(cfgmap, "name", &RE_COND_NAME))?.unwrap();

        // specific mandatory parameter retrieval
        let bus =
            cfg_mandatory!(cfg_string_check_regex(cfgmap, "bus", &RE_DBUS_MSGBUS_NAME))?.unwrap();
        let service = cfg_mandatory!(cfg_string_check_regex(
            cfgmap,
            "service",
            &RE_DBUS_SERVICE_NAME
        ))?
        .unwrap();
        let object_path = cfg_mandatory!(cfg_string_check_regex(
            cfgmap,
            "object_path",
            &RE_DBUS_OBJECT_PATH
        ))?
        .unwrap();
        let interface = cfg_mandatory!(cfg_string_check_regex(
            cfgmap,
            "interface",
            &RE_DBUS_INTERFACE_NAME
        ))?
        .unwrap();
        let method = cfg_mandatory!(cfg_string_check_regex(
            cfgmap,
            "method",
            &RE_DBUS_MEMBER_NAME
        ))?
        .unwrap();

        // initialize the structure
        let mut new_condition = DbusMethodCondition::new(&name);
        new_condition.task_registry = Some(task_registry);
        new_condition.bus = Some(bus);
        new_condition.service = Some(service);
        new_condition.object_path = Some(object_path);
        new_condition.interface = Some(interface);
        new_condition.method = Some(method);

        // by default make condition active if loaded from configuration: if
        // the configuration changes this state the condition will not start
        new_condition.suspended = false;

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

        // retrieve task list and try to directly add each task
        if let Some(v) = cfg_vec_string_check_regex(cfgmap, "tasks", &RE_TASK_NAME)? {
            for s in v {
                if !new_condition.add_task(&s)? {
                    return Err(cfg_err_invalid_config(cur_key, &s, ERR_INVALID_TASK));
                }
            }
        }

        if let Some(v) = cfg_bool(cfgmap, "recurring")? {
            new_condition.recurring = v;
        }
        if let Some(v) = cfg_int_check_above_eq(cfgmap, "max_tasks_retries", -1)? {
            new_condition.max_retries = v;
        }
        if let Some(v) = cfg_bool(cfgmap, "execute_sequence")? {
            new_condition.exec_sequence = v;
        }
        if let Some(v) = cfg_bool(cfgmap, "break_on_failure")? {
            new_condition.break_on_failure = v;
        }
        if let Some(v) = cfg_bool(cfgmap, "break_on_success")? {
            new_condition.break_on_success = v;
        }
        if let Some(v) = cfg_bool(cfgmap, "suspended")? {
            new_condition.suspended = v;
        }

        // specific optional parameter initialization
        if let Some(v) = cfg_int_check_above_eq(cfgmap, "check_after", 1)? {
            new_condition.check_after = Some(Duration::from_secs(v as u64));
        }
        if let Some(v) = cfg_bool(cfgmap, "recur_after_failed_check")? {
            new_condition.recur_after_failed_check = v;
        }

        // this is tricky: we build a list of elements constituted by:
        // - an index list (integers and strings, mixed) which will address
        //   every nested structure,
        // - an operator,
        // - a value to check against using the operator;
        // of course the value types found in TOML are less tructured than the
        // ones supported by DBus, and subsequent tests will take this into
        // account and compare only values compatible with each other, and
        // compatible with the operator used
        let check = ["index", "operator", "value"];
        let cur_key = "parameter_check";
        if let Some(item) = cfgmap.get(cur_key) {
            let mut param_checks: Vec<ParameterCheckTest> = Vec::new();
            let params = if !item.is_list() {
                return Err(cfg_err_invalid_config(
                    cur_key,
                    STR_UNKNOWN_VALUE,
                    ERR_INVALID_VALUE_FOR_ENTRY,
                ));
            } else {
                item.clone()
            };
            let item = params.as_list().unwrap();
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
                            let re = Regex::new(s);
                            if let Ok(re) = re {
                                value = ParameterCheckValue::Regex(re);
                            } else {
                                return Err(cfg_err_invalid_config(
                                    &format!("{cur_key}:value"),
                                    STR_UNKNOWN_VALUE,
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
            // into the new condition structure: the list is formally correct,
            // but it may not be compatible with the returned parameters, in
            // which case the parameter check will evaluate to _non-verified_
            // and a warning log message will be issued (see below)
            new_condition.param_checks = Some(param_checks);

            // `parameter_check_all` only makes sense if the parameter check
            // list was built: for this reason it is set only in this case
            if let Some(v) = cfg_bool(cfgmap, "parameter_check_all")? {
                new_condition.param_checks_all = v;
            }
        }

        // here we must build a list of `zvariant::Value` objects, which are
        // dynamic: the list will be formally a valid parameter list but it is
        // not assured to be compatible with the called method (that is, no
        // check against signature is made here); in case of incompatibility
        // the condition evaluation will (always) fail and a warning will be
        // logged
        let cur_key = "parameter_call";
        if let Some(item) = cfgmap.get(cur_key) {
            let mut param_call: Vec<zvariant::OwnedValue> = Vec::new();
            let params = if !item.is_list() {
                return Err(cfg_err_invalid_config(
                    cur_key,
                    STR_UNKNOWN_VALUE,
                    ERR_INVALID_VALUE_FOR_ENTRY,
                ));
            } else {
                item.clone()
            };
            let item = params.as_list().unwrap();
            // the `ToVariant` trait should do the tedious recursive job for
            // us: should there be any unsupported value in the array the
            // result will be None and the configuration is rejectesd
            for i in item.iter() {
                let v = i.to_variant();
                if let Some(v) = v {
                    let v = v.try_to_owned();
                    if v.is_err() {
                        return Err(cfg_err_invalid_config(
                            cur_key,
                            STR_UNKNOWN_VALUE,
                            ERR_INVALID_CONFIG_FOR_ENTRY,
                        ));
                    } else {
                        param_call.push(v.unwrap());
                    }
                } else {
                    return Err(cfg_err_invalid_config(
                        cur_key,
                        STR_UNKNOWN_VALUE,
                        ERR_INVALID_VALUE_FOR_ENTRY,
                    ));
                }
            }
            // the parameters for the message invocation can now be set
            new_condition.param_call = Some(param_call);
        }

        // start the condition if the configuration did not suspend it
        if !new_condition.suspended {
            new_condition.start()?;
        }

        Ok(new_condition)
    }

    /// Check a configuration map and return item name if Ok
    ///
    /// The check is performed exactly in the same way and in the same order
    /// as in `load_cfgmap`, the only difference is that no actual item is
    /// created and that a name is returned, which is the name of the item that
    /// _would_ be created via the equivalent call to `load_cfgmap`
    pub fn check_cfgmap(cfgmap: &CfgMap, available_tasks: &Vec<&str>) -> Result<String> {
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
            "tasks",
            "recurring",
            "max_tasks_retries",
            "execute_sequence",
            "break_on_failure",
            "break_on_success",
            "suspended",
            "check_after",
            "bus",
            "service",
            "object_path",
            "interface",
            "method",
            "parameter_call",
            "parameter_check_all",
            "parameter_check",
            "recur_after_failed_check",
        ];
        cfg_check_keys(cfgmap, &check)?;

        // common mandatory parameter check

        // type and name are both mandatory: type is checked and name is kept
        cfg_mandatory!(cfg_string_check_exact(cfgmap, "type", "dbus"))?;
        let name = cfg_mandatory!(cfg_string_check_regex(cfgmap, "name", &RE_COND_NAME))?.unwrap();

        // specific mandatory parameter check
        cfg_mandatory!(cfg_string_check_regex(cfgmap, "bus", &RE_DBUS_MSGBUS_NAME))?;
        cfg_mandatory!(cfg_string_check_regex(
            cfgmap,
            "service",
            &RE_DBUS_SERVICE_NAME
        ))?;
        cfg_mandatory!(cfg_string_check_regex(
            cfgmap,
            "object_path",
            &RE_DBUS_OBJECT_PATH
        ))?;
        cfg_mandatory!(cfg_string_check_regex(
            cfgmap,
            "interface",
            &RE_DBUS_INTERFACE_NAME
        ))?;
        cfg_mandatory!(cfg_string_check_regex(
            cfgmap,
            "method",
            &RE_DBUS_MEMBER_NAME
        ))?;

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

        // check configuration task list against the provided ones
        if let Some(v) = cfg_vec_string_check_regex(cfgmap, "tasks", &RE_TASK_NAME)? {
            for s in v {
                if !available_tasks.contains(&s.as_str()) {
                    return Err(cfg_err_invalid_config(cur_key, &s, ERR_INVALID_TASK));
                }
            }
        }

        cfg_bool(cfgmap, "recurring")?;
        cfg_int_check_above_eq(cfgmap, "max_tasks_retries", -1)?;
        cfg_bool(cfgmap, "execute_sequence")?;
        cfg_bool(cfgmap, "break_on_failure")?;
        cfg_bool(cfgmap, "break_on_success")?;
        cfg_bool(cfgmap, "suspended")?;

        cfg_int_check_above_eq(cfgmap, "check_after", 1)?;
        cfg_bool(cfgmap, "recur_after_failed_check")?;

        // this is tricky: we build a list of elements constituted by:
        // - an index list (integers and strings, mixed) which will address
        //   every nested structure,
        // - an operator,
        // - a value to check against using the operator;
        // of course the value types found in TOML are less tructured than the
        // ones supported by DBus, and subsequent tests will take this into
        // account and compare only values compatible with each other, and
        // compatible with the operator used
        let check = ["index", "operator", "value"];

        let cur_key = "parameter_check";
        if let Some(item) = cfgmap.get(cur_key) {
            let params = if !item.is_list() {
                return Err(cfg_err_invalid_config(
                    cur_key,
                    STR_UNKNOWN_VALUE,
                    ERR_INVALID_VALUE_FOR_ENTRY,
                ));
            } else {
                item.clone()
            };
            let item = params.as_list().unwrap();
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
                            &format!("{oper:?}"),
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
                            &format!("{v:?}"),
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

            // `parameter_check_all` only makes sense if the paramenter check
            // list was built: for this reason it is checked only in this case
            // (so that the checks are the same as the ones in load_cfgmap)
            cfg_bool(cfgmap, "parameter_check_all")?;
        }

        // here we must build a list of `zvariant::Value` objects, which are
        // dynamic: the list will be formally a valid parameter list but it is
        // not assured to be compatible with the called method (that is, no
        // check against signature is made here); in case of incompatibility
        // the condition evaluation will (always) fail and a warning will be
        // logged
        let cur_key = "parameter_call";
        if let Some(item) = cfgmap.get(cur_key) {
            let params = if !item.is_list() {
                return Err(cfg_err_invalid_config(
                    cur_key,
                    STR_UNKNOWN_VALUE,
                    ERR_INVALID_VALUE_FOR_ENTRY,
                ));
            } else {
                item.clone()
            };
            let item = params.as_list().unwrap();
            // the `ToVariant` trait should do the tedious recursive job for
            // us: should there be any unsupported value in the array the
            // result will be None and the configuration is rejectesd
            for i in item.iter() {
                let v = i.to_variant();
                if v.is_none() {
                    return Err(cfg_err_invalid_config(
                        cur_key,
                        STR_UNKNOWN_VALUE,
                        ERR_INVALID_VALUE_FOR_ENTRY,
                    ));
                }
            }
        }

        Ok(name)
    }
}

impl Condition for DbusMethodCondition {
    fn set_id(&mut self, id: i64) {
        self.cond_id = id;
    }
    fn get_name(&self) -> String {
        self.cond_name.clone()
    }
    fn get_id(&self) -> i64 {
        self.cond_id
    }
    fn get_type(&self) -> &str {
        "dbus"
    }

    /// Return a hash of this item for comparison
    fn _hash(&self) -> u64 {
        let mut s = DefaultHasher::new();
        self.hash(&mut s);
        s.finish()
    }

    fn set_task_registry(&mut self, reg: &'static TaskRegistry) {
        self.task_registry = Some(reg);
    }

    fn task_registry(&self) -> Option<&'static TaskRegistry> {
        self.task_registry
    }

    fn suspended(&self) -> bool {
        self.suspended
    }
    fn recurring(&self) -> bool {
        self.recurring
    }
    fn has_succeeded(&self) -> bool {
        self.has_succeeded
    }

    fn exec_sequence(&self) -> bool {
        self.exec_sequence
    }
    fn break_on_success(&self) -> bool {
        self.break_on_success
    }
    fn break_on_failure(&self) -> bool {
        self.break_on_failure
    }

    fn last_checked(&self) -> Option<Instant> {
        self.last_tested
    }
    fn last_succeeded(&self) -> Option<Instant> {
        self.last_succeeded
    }
    fn startup_time(&self) -> Option<Instant> {
        self.startup_time
    }

    fn set_checked(&mut self) -> Result<bool> {
        self.last_tested = Some(Instant::now());
        Ok(true)
    }

    fn set_succeeded(&mut self) -> Result<bool> {
        self.last_succeeded = self.last_tested;
        self.has_succeeded = true;
        Ok(true)
    }

    fn reset_succeeded(&mut self) -> Result<bool> {
        self.last_succeeded = None;
        self.has_succeeded = false;
        Ok(true)
    }

    fn reset(&mut self) -> Result<bool> {
        self.last_tested = None;
        self.last_succeeded = None;
        self.has_succeeded = false;
        self.left_retries = self.max_retries + 1;
        self.tasks_failed = true;
        Ok(true)
    }

    fn left_retries(&self) -> Option<i64> {
        if self.max_retries == -1 {
            None
        } else {
            Some(self.left_retries)
        }
    }

    fn set_retried(&mut self) {
        if self.left_retries > 0 {
            self.left_retries -= 1;
        }
    }

    fn start(&mut self) -> Result<bool> {
        self.suspended = false;
        self.left_retries = self.max_retries + 1;
        self.startup_time = Some(Instant::now());

        // set the tasks_failed flag upon start: no task has been run now
        // and this is equivalent to a failure; the flag was set to `false`
        // upon creation in order to have a zero-initialization
        self.tasks_failed = true;
        Ok(true)
    }

    fn suspend(&mut self) -> Result<bool> {
        if self.suspended {
            Ok(false)
        } else {
            self.suspended = true;
            Ok(true)
        }
    }

    fn resume(&mut self) -> Result<bool> {
        if self.suspended {
            self.suspended = false;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn task_names(&self) -> Result<Vec<String>> {
        Ok(self.task_names.clone())
    }

    fn any_tasks_failed(&self) -> bool {
        self.tasks_failed
    }

    fn set_tasks_failed(&mut self, failed: bool) {
        self.tasks_failed = failed;
    }

    fn _add_task(&mut self, name: &str) -> Result<bool> {
        let name = String::from(name);
        if self.task_names.contains(&name) {
            Ok(false)
        } else {
            self.task_names.push(name);
            Ok(true)
        }
    }

    fn _remove_task(&mut self, name: &str) -> Result<bool> {
        let name = String::from(name);
        if self.task_names.contains(&name) {
            self.task_names
                .remove(self.task_names.iter().position(|x| x == &name).unwrap());
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Mandatory check function.
    ///
    /// This function actually performs the test.
    fn _check_condition(&mut self) -> Result<Option<bool>> {
        // NOTE: the following helpers are async here, but since this check
        //       runs in a dedicated thread, we will just block on every step;
        //       the zbus::blocking option might be considered for further
        //       developement as well as rebuilding this service as real async

        // panic here if the bus name is incorrect: should have been fixed
        // when the condition was configured and constructed
        async fn _get_connection(bus: &str) -> zbus::Result<zbus::Connection> {
            let connection;
            if bus == ":session" {
                connection = zbus::Connection::session().await;
            } else if bus == ":system" {
                connection = zbus::Connection::system().await;
            } else {
                panic!("specified bus `{bus}` not supported for DBus method based condition");
            }
            connection
        }

        self.log(
            LogType::Debug,
            LOG_WHEN_START,
            LOG_STATUS_MSG,
            "checking DBus method based condition",
        );
        // if the minimum interval between checks has been set, obey it
        // last_tested has already been set by trait to Instant::now()
        let t = self.last_tested.unwrap();
        if let Some(e) = self.check_after {
            if e > t - self.check_last {
                self.log(
                    LogType::Debug,
                    LOG_WHEN_START,
                    LOG_STATUS_MSG,
                    "check explicitly delayed by configuration",
                );
                return Ok(Some(false));
            }
        }

        // first unwrap all data needed to install the listening service:
        // this panics if any of the data is uninitialized because all the
        // mandatory parameters must be set, and any missing value would
        // indicate that there is a mistake in the program flow
        let bus = self
            .bus
            .clone()
            .expect("attempt to check condition with uninitialized bus");

        let service = self
            .service
            .clone()
            .expect("attempt to check condition with uninitialized service");

        let object_path = self
            .object_path
            .clone()
            .expect("attempt to check condition with uninitialized object path");

        let interface = self
            .interface
            .clone()
            .expect("attempt to check condition with uninitialized interface");

        let method = self
            .method
            .clone()
            .expect("attempt to check condition with uninitialized method");

        // connect to the DBus service
        self.log(
            LogType::Debug,
            LOG_WHEN_START,
            LOG_STATUS_MSG,
            &format!("opening connection to bus `{bus}`"),
        );
        let conn = task::block_on(async { _get_connection(&bus).await })?;

        // the following bifurcation is in my opinion quite ugly, however it
        // looks like the entire `zbus` crate is built with the purpose of
        // providing a way to proxy known existing DBus methods at compile
        // time, and thus I did not found a way to call a method without
        // arguments if not by passing a pointer to the unit as argument to
        // the `call_method` function
        let message;
        if let Some(params) = self.param_call.clone() {
            let mut arg = zvariant::StructureBuilder::new();
            for p in params {
                let v = zvariant::Value::from(p);
                arg.push_value(v);
            }
            if let Ok(arg) = arg.build() {
                message = task::block_on(async {
                    conn.call_method(
                        if service.is_empty() {
                            None
                        } else {
                            Some(service.as_str())
                        },
                        object_path.as_str(),
                        if interface.is_empty() {
                            None
                        } else {
                            Some(interface.as_str())
                        },
                        method.as_str(),
                        &arg,
                    ).await
                })
            } else {
                self.log(
                    LogType::Warn,
                    LOG_WHEN_START,
                    LOG_STATUS_FAIL,
                    &format!("could not build parameter list invoking method {method} on bus `{bus}`"),
                );
                return Ok(Some(false));
            };
        } else {
            message = task::block_on(async {
                conn.call_method(
                    if service.is_empty() {
                        None
                    } else {
                        Some(service.as_str())
                    },
                    object_path.as_str(),
                    if interface.is_empty() {
                        None
                    } else {
                        Some(interface.as_str())
                    },
                    method.as_str(),
                    &(),
                )
                .await
            });
        }
        if let Err(e) = message {
            self.log(
                LogType::Warn,
                LOG_WHEN_START,
                LOG_STATUS_FAIL,
                &format!("could not retrieve message invoking method {method} on bus `{bus}`: {e}"),
            );
            return Ok(Some(false));
        }
        let message = message.unwrap();

        // now check method result in the same way as in signal message
        let verified;
        if let Some(checks) = &self.param_checks {
            self.log(
                LogType::Debug,
                LOG_WHEN_PROC,
                LOG_STATUS_MSG,
                &format!("parameter checks specified: {} checks must be verified", {
                    if self.param_checks_all { "all" } else { "some" }
                }),
            );

            let severity;
            let log_when;
            let log_status;
            let log_message;

            (verified, severity, log_when, log_status, log_message) =
                dbus_check_message(&message, checks, self.param_checks_all);
            self.log(severity, log_when, log_status, &log_message);
        } else {
            panic!("attempt to verify condition without initializing tests")
        }

        // now the time of the last check can be set to the actual time in
        // order to allow further checks to comply with the request to be
        // only run at certain intervals
        self.check_last = t;

        // prevent success if in persistent success state and status change
        // is required to succeed again
        let can_succeed = self.last_check_failed || !self.recur_after_failed_check;
        self.last_check_failed = !verified;
        if !can_succeed && verified {
            self.log(
                LogType::Debug,
                LOG_WHEN_END,
                LOG_STATUS_MSG,
                "persistent success status: waiting for failure to recur",
            );
        }

        Ok(Some(verified && can_succeed))
    }
}

// end.
