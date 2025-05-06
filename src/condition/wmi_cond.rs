//! Define a WMI query interrogation based condition
//!
//! This condition is verified whenever the given query returns a result that
//! matches the provided criteria: criteria are specified as a list of
//! `ResultCheckTest` entries.
//!
//! A well crafted query might also not need to return a set of records, even
//! a single value could be sufficient. As in similar multiple-criteria items
//! it is possible to specify that either just one or all the criteria have
//! to be met.

#![cfg(windows)]
#![cfg(feature = "wmi")]

use std::hash::{DefaultHasher, Hash, Hasher};
use std::time::{Duration, Instant};

use cfgmap::CfgMap;
use regex::Regex;
use std::collections::HashMap;

use wmi::{COMLibrary, Variant, WMIConnection};

use super::base::Condition;
use crate::common::logging::{LogType, log};
use crate::common::wmiitem::*;
use crate::common::wres::Result;
use crate::task::registry::TaskRegistry;
use crate::{cfg_mandatory, constants::*};

use crate::cfghelp::*;

/// WMI Query Based Condition
///
/// This condition is verified whenever one or more rows returned by a WMI
/// query satisfy the provided criteria.
pub struct WmiQueryCondition {
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
    query: Option<String>,
    result_checks: Option<Vec<ResultCheckTest>>,
    result_checks_all: bool,
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
impl Hash for WmiQueryCondition {
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
        self.query.hash(state);
        self.result_checks.hash(state);
        self.result_checks_all.hash(state);
        self.check_after.hash(state);
        self.recur_after_failed_check.hash(state);
    }
}

#[allow(dead_code)]
impl WmiQueryCondition {
    /// Create a new WMI query invocation based condition with the given name
    pub fn new(name: &str) -> Self {
        log(
            LogType::Debug,
            LOG_EMITTER_CONDITION_WMI,
            LOG_ACTION_NEW,
            Some((name, 0)),
            LOG_WHEN_INIT,
            LOG_STATUS_MSG,
            &format!("CONDITION {name}: creating a new WMI query based condition"),
        );
        let t = Instant::now();
        WmiQueryCondition {
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
            query: None,
            result_checks: None,
            result_checks_all: false,
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

    /// Set the query: no validity check is performed
    pub fn set_query(&mut self, query: &str) -> bool {
        self.query = Some(String::from(query));
        return true;
    }

    /// Return an owned copy of the query
    pub fn query(&self) -> Option<String> {
        self.query.clone()
    }

    /// Load a `WmiQueryCondition` from a [`CfgMap`](https://docs.rs/cfgmap/latest/)
    ///
    /// The `WmiQueryCondition` is initialized according to the values
    /// provided in the `CfgMap` argument. If the `CfgMap` format does not
    /// comply with the requirements of a `WmiQueryCondition` an error is
    /// raised.
    ///
    /// The values for the `result_check` entries are provided as a list of
    /// dictionaries, because the WMI case is quite simple to handle: each
    /// test dictionary will contain an index (optional) to specify which
    /// returned record has to be tonsidered, a field name, an operator, and
    /// a value for comparison.
    pub fn load_cfgmap(
        cfgmap: &CfgMap,
        task_registry: &'static TaskRegistry,
    ) -> Result<WmiQueryCondition> {
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
            "query",
            "result_check_all",
            "result_check",
            "recur_after_failed_check",
        ];
        cfg_check_keys(cfgmap, &check)?;

        // common mandatory parameter retrieval

        // type and name are both mandatory but type is only checked
        cfg_mandatory!(cfg_string_check_exact(cfgmap, "type", "wmi"))?;
        let name = cfg_mandatory!(cfg_string_check_regex(cfgmap, "name", &RE_COND_NAME))?.unwrap();

        // specific mandatory parameter retrieval
        let query = cfg_mandatory!(cfg_string(cfgmap, "query"))?.unwrap();

        // initialize the structure
        let mut new_condition = WmiQueryCondition::new(&name);
        new_condition.task_registry = Some(task_registry);
        new_condition.query = Some(query);

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

        // here the list of result checks is built
        let check = ["index", "field", "operator", "value"];
        let cur_key = "result_check";
        if let Some(item) = cfgmap.get(cur_key) {
            let mut result_checks: Vec<ResultCheckTest> = Vec::new();
            // here we expect a list of simple maps
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

                let index = cfg_int_check_above_eq(spec, "index", 0);
                if index.is_err() {
                    return Err(cfg_err_invalid_config(
                        &format!("{cur_key}:index"),
                        &format!("{index:?}"),
                        ERR_INVALID_VALUE_FOR_ENTRY,
                    ));
                }
                let index = index.unwrap();

                let field = cfg_mandatory!(cfg_string_check_regex(spec, "field", &RE_VAR_NAME));
                if field.is_err() {
                    return Err(cfg_err_invalid_config(
                        &format!("{cur_key}:field"),
                        &format!("{field:?}"),
                        ERR_INVALID_VALUE_FOR_ENTRY,
                    ));
                }
                let field = field.unwrap().unwrap();

                let operator;
                if let Some(oper) = spec.get("operator") {
                    if oper.is_str() {
                        operator = match oper.as_str().unwrap().as_str() {
                            "eq" => ResultCheckOperator::Equal,
                            "neq" => ResultCheckOperator::NotEqual,
                            "gt" => ResultCheckOperator::Greater,
                            "ge" => ResultCheckOperator::GreaterEqual,
                            "lt" => ResultCheckOperator::Less,
                            "le" => ResultCheckOperator::LessEqual,
                            "match" => ResultCheckOperator::Match,
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

                let value;
                if let Some(v) = spec.get("value") {
                    if v.is_bool() {
                        value = ResultCheckValue::Boolean(*v.as_bool().unwrap());
                    } else if v.is_int() {
                        value = ResultCheckValue::Integer(*v.as_int().unwrap());
                    } else if v.is_float() {
                        value = ResultCheckValue::Float(*v.as_float().unwrap());
                    } else if v.is_str() {
                        let s = v.as_str().unwrap();
                        if operator == ResultCheckOperator::Match {
                            let re = Regex::new(s);
                            if let Ok(re) = re {
                                value = ResultCheckValue::Regex(re);
                            } else {
                                return Err(cfg_err_invalid_config(
                                    &format!("{cur_key}:value"),
                                    &format!("{v:?}"),
                                    ERR_INVALID_VALUE_FOR_ENTRY,
                                ));
                            }
                        } else {
                            value = ResultCheckValue::String(s.to_string());
                        }
                    } else {
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
                // now that we have the full struct, we can add it to criteria
                result_checks.push(ResultCheckTest {
                    index: if let Some(i) = index {
                        Some(i as usize)
                    } else {
                        None
                    },
                    field,
                    operator,
                    value,
                });
            }
            // finally the parameter checks become `Some` and makes its way
            // into the new condition structure: the list is formally correct,
            // but it may not be compatible with the returned parameters, in
            // which case the parameter check will evaluate to _non-verified_
            // and a warning log message will be issued (see below)
            new_condition.result_checks = Some(result_checks);

            // `result_check_all` only makes sense if the parameter check
            // list was built: for this reason it is set only in this case
            if let Some(v) = cfg_bool(cfgmap, "result_check_all")? {
                new_condition.result_checks_all = v;
            }
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
            "query",
            "result_check_all",
            "result_check",
            "recur_after_failed_check",
        ];
        cfg_check_keys(cfgmap, &check)?;

        // common mandatory parameter check

        // type and name are both mandatory: type is checked and name is kept
        cfg_mandatory!(cfg_string_check_exact(cfgmap, "type", "wmi"))?;
        let name = cfg_mandatory!(cfg_string_check_regex(cfgmap, "name", &RE_COND_NAME))?.unwrap();

        // specific mandatory parameter check
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

        // here the list of result checks is verified
        let check = ["index", "field", "operator", "value"];
        let cur_key = "result_check";
        if let Some(item) = cfgmap.get(cur_key) {
            // here we expect a list of simple maps
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

                let index = cfg_int_check_above_eq(spec, "index", 0);
                if index.is_err() {
                    return Err(cfg_err_invalid_config(
                        &format!("{cur_key}:index"),
                        &format!("{index:?}"),
                        ERR_INVALID_VALUE_FOR_ENTRY,
                    ));
                }

                let field = cfg_mandatory!(cfg_string_check_regex(spec, "field", &RE_VAR_NAME));
                if field.is_err() {
                    return Err(cfg_err_invalid_config(
                        &format!("{cur_key}:field"),
                        &format!("{field:?}"),
                        ERR_INVALID_VALUE_FOR_ENTRY,
                    ));
                }

                let operator;
                if let Some(oper) = spec.get("operator") {
                    if oper.is_str() {
                        operator = match oper.as_str().unwrap().as_str() {
                            "eq" => ResultCheckOperator::Equal,
                            "neq" => ResultCheckOperator::NotEqual,
                            "gt" => ResultCheckOperator::Greater,
                            "ge" => ResultCheckOperator::GreaterEqual,
                            "lt" => ResultCheckOperator::Less,
                            "le" => ResultCheckOperator::LessEqual,
                            "match" => ResultCheckOperator::Match,
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
                        if operator == ResultCheckOperator::Match {
                            let re = Regex::new(s);
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

            // `result_check_all` only makes sense if the paramenter check
            // list was built: for this reason it is checked only in this case
            // (so that the checks are the same as the ones in load_cfgmap)
            cfg_bool(cfgmap, "result_check_all")?;
        }

        Ok(name)
    }
}

impl Condition for WmiQueryCondition {
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
        "wmi"
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

    fn _check_condition(&mut self) -> Result<Option<bool>> {
        self.log(
            LogType::Debug,
            LOG_WHEN_START,
            LOG_STATUS_MSG,
            "checking WMI query based condition",
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

        // run the WMI query and retrieve results, then perform the checks
        // using the helper method provided in common::wmiitem; the query is
        // executed anyway, even in case no checks have been provided, because
        // it still can produce an error or an empty result, where the latter
        // case is considered a failure
        let com_lib = COMLibrary::new()?;
        let conn = WMIConnection::new(com_lib)?;

        let query = self.query.clone().unwrap();
        let results: Vec<HashMap<String, Variant>> = conn.raw_query(query)?;
        if results.is_empty() {
            self.log(
                LogType::Debug,
                LOG_WHEN_END,
                LOG_STATUS_MSG,
                "check failed because the query produced no results",
            );
            return Ok(Some(false));
        }

        let verified;
        if let Some(checks) = &self.result_checks {
            // if no checks are given, the result is always successful: in
            // this way one can just run a query, regardless of its results,
            // and consider the condition satisfied if it just returns
            // anything without error
            if checks.is_empty() {
                self.log(
                    LogType::Debug,
                    LOG_WHEN_END,
                    LOG_STATUS_MSG,
                    "no result checks specified: outcome is success",
                );
                return Ok(Some(true));
            }

            // otherwise, log and perform the tests
            self.log(
                LogType::Debug,
                LOG_WHEN_PROC,
                LOG_STATUS_MSG,
                &format!("result checks specified: {} checks must be verified", {
                    if self.result_checks_all {
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
                wmi_check_result(&results, &checks, self.result_checks_all);
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
                &"persistent success status: waiting for failure to recur".to_string(),
            );
        }

        Ok(Some(verified && can_succeed))
    }
}

// end.
