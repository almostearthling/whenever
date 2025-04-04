//! Define a _Lua_ script based condition
//!
//! This `Condition` executes a _Lua_ script provided in the configuration,
//! then checks the contents of one or more variables to match the expected
//! result and states success or failure accordingly. An error in the script
//! always results as a failure.


use std::collections::HashMap;
use std::time::{Instant, SystemTime, Duration};
use std::hash::{DefaultHasher, Hash, Hasher};

use itertools::Itertools;

use cfgmap::CfgMap;
use mlua;

use super::base::Condition;
use crate::task::registry::TaskRegistry;
use crate::common::logging::{log, LogType};
use crate::common::wres::{Error, Kind, Result};
use crate::common::luaitem::*;
use crate::{cfg_mandatory, constants::*};

use crate::cfghelp::*;



/// _Lua_ script Based Condition
///
/// This condition is verified when the underlying _Lua_ script execution
/// outcome meets the criteria given at construction time.
pub struct LuaCondition {
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
    script: String,
    set_vars: bool,
    expected: HashMap<String, LuaValue>,
    expect_all: bool,
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
impl Hash for LuaCondition {
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
        self.script.hash(state);
        self.set_vars.hash(state);
        self.expect_all.hash(state);
        self.recur_after_failed_check.hash(state);

        // expected values is sorted because the order in which they are
        // defined is not significant
        for key in self.expected.keys().sorted() {
            key.hash(state);
            match &self.expected[key] {
                LuaValue::LuaBoolean(x) => x.hash(state),
                LuaValue::LuaNumber(x) => x.to_bits().hash(state),
                LuaValue::LuaString(x) => x.hash(state),
            }
        }
    }
}


#[allow(dead_code)]
impl LuaCondition {

    /// Create a new _Lua_ script based condition with the given name and
    /// the specified _Lua_ script provided as a string
    pub fn new(
        name: &str,
        script: &str,
    ) -> Self {
        log(
            LogType::Debug,
            LOG_EMITTER_CONDITION_LUA,
            LOG_ACTION_NEW,
            Some((name, 0)),
            LOG_WHEN_INIT,
            LOG_STATUS_MSG,
            &format!("CONDITION {name}: creating a new Lua script based condition"),
        );
        let t = Instant::now();
        LuaCondition {
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
            script: String::from(script),
            set_vars: true,
            expected: HashMap::new(),
            expect_all: false,
            recur_after_failed_check: false,
            check_after: None,

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

    /// Add a variable to check for a string value
    pub fn add_check_string(mut self, varname: &str, value: &str) -> Self {
        self.expected.insert(varname.to_string(), LuaValue::LuaString(value.to_string()));
        self
    }

    /// Add a variable to check for a number (f64) value
    pub fn add_check_number(mut self, varname: &str, value: f64) -> Self {
        self.expected.insert(varname.to_string(), LuaValue::LuaNumber(value));
        self
    }

    /// Add a variable to check for a boolean value
    pub fn add_check_bool(mut self, varname: &str, value: bool) -> Self {
        self.expected.insert(varname.to_string(), LuaValue::LuaBoolean(value));
        self
    }

    /// Constructor modifier that states that all variable values has to be
    /// matched for success. Default behaviour is that if at least one of the
    /// checks succeed then the result is successful.
    pub fn checks_all(mut self, yes: bool) -> Self {
        self.expect_all = yes;
        self
    }


    /// Constructor modifier to specify that the condition should not set the
    /// context variables that specify the task name and the condition that
    /// triggered the task, when set to `false`. The default behaviour is to
    /// export those variables.
    pub fn sets_vars(mut self, yes: bool) -> Self {
        self.set_vars = yes;
        self
    }


    /// Constructor modifier to specify that the condition is verified on
    /// check success only if there has been at least one failure after the
    /// last successful test
    pub fn recurs_after_check_failure(mut self, yes: bool) -> Self {
        self.recur_after_failed_check = yes;
        self
    }


    /// State that the first check and possible following tests are to be
    /// performed after a certain amount of time. This option is present in
    /// this type of condition because the test itself can be both time and
    /// resource consuming, and an user may choose to avoid to perform it
    /// at every tick.
    pub fn checks_after(mut self, delta: Duration) -> Self {
        self.check_after = Some(delta);
        self
    }


    // helper to build a representation of checks for logging
    fn repr_checks(&self) -> String {
        let mut res = String::new();
        let sep = { if self.expect_all { "and" } else { "or" } };
        for (k, v) in self.expected.iter() {
            let rval = match v {
                LuaValue::LuaString(v) => format!("\"{v}\""),
                LuaValue::LuaNumber(v) => format!("{v:.2}"),
                LuaValue::LuaBoolean(v) => format!("{v}"),
            };
            if !res.is_empty() {
                res = format!("{res} {sep} {k}={rval}");
            } else {
                res = format!("{k}={rval}");
            }
        }
        res
    }

    /// Load a `LuaCondition` from a [`CfgMap`](https://docs.rs/cfgmap/latest/)
    ///
    /// The `LuaCondition` is initialized according to the values provided
    /// in the `CfgMap` argument. If the `CfgMap` format does not comply with
    /// the requirements of a `LuaCondition` an error is raised.
    pub fn load_cfgmap(cfgmap: &CfgMap, task_registry: &'static TaskRegistry) -> Result<LuaCondition> {

        // fn _invalid_cfg(key: &str, value: &str, message: &str) -> Result<LuaCondition> {
        //     Err(Error::new(
        //         Kind::Invalid,
        //         &format!("{ERR_INVALID_COND_CONFIG}: ({key}={value}) {message}"),
        //     ))
        // }

        let check = vec![
            "type",
            "name",
            "tags",
            "script",
            "tasks",
            "recurring",
            "max_tasks_retries",
            "execute_sequence",
            "break_on_failure",
            "break_on_success",
            "suspended",
            "expect_all",
            "recur_after_failed_check",
            "expected_results",
            "check_after",
        ];
        cfg_check_keys(cfgmap, &check)?;

        // common mandatory parameter retrieval

        // type and name are both mandatory but type is only checked
        cfg_mandatory!(cfg_string_check_exact(cfgmap, "type", "lua"))?;
        let name = cfg_mandatory!(cfg_string_check_regex(cfgmap, "name", &RE_COND_NAME))?.unwrap();

        // specific mandatory parameter retrieval
        let script = String::from(cfg_mandatory!(cfg_string(cfgmap, "script"))?.unwrap());

        // initialize the structure
        let mut new_condition = LuaCondition::new(
            &name,
            &script,
        );
        new_condition.task_registry = Some(task_registry);

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
                    return Err(cfg_err_invalid_config(
                        cur_key,
                        &s,
                        ERR_INVALID_TASK,
                    ));
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
        if let Some(v) = cfg_bool(cfgmap, "expect_all")? {
            new_condition.expect_all = v;
        }
        if let Some(v) = cfg_bool(cfgmap, "recur_after_failed_check")? {
            new_condition.recur_after_failed_check = v;
        }

        // expected results are in a complex map, thus no shortcut is given
        let cur_key = "expected_results";
        if cfgmap.contains_key(cur_key) {
            if let Some(item) = cfgmap.get(cur_key) {
                if !item.is_map() {
                    return Err(cfg_err_invalid_config(
                        cur_key,
                        STR_UNKNOWN_VALUE,
                        ERR_INVALID_PARAMETER,
                    ));
                } else {
                    let map = item.as_map().unwrap();
                    let mut vars: HashMap<String, LuaValue> = HashMap::new();
                    for name in map.keys() {
                        if !RE_VAR_NAME.is_match(name) {
                            return Err(cfg_err_invalid_config(
                                cur_key,
                                &name,
                                ERR_INVALID_VAR_NAME,
                            ));
                        } else if let Some(value) = map.get(name) {
                            if value.is_int() {
                                let v = value.as_int().unwrap();
                                vars.insert(name.to_string(), LuaValue::LuaNumber(*v as f64));
                            } else if value.is_float() {
                                let v = value.as_float().unwrap();
                                vars.insert(name.to_string(), LuaValue::LuaNumber(*v));
                            } else if value.is_bool() {
                                let v = value.as_bool().unwrap();
                                vars.insert(name.to_string(), LuaValue::LuaBoolean(*v));
                            } else if value.is_str() {
                                let v = value.as_str().unwrap();
                                vars.insert(name.to_string(), LuaValue::LuaString(v.to_string()));
                            } else {
                                return Err(cfg_err_invalid_config(
                                    cur_key,
                                    STR_UNKNOWN_VALUE,
                                    ERR_INVALID_VAR_VALUE,
                                ));
                            }
                        } else {
                            return Err(cfg_err_invalid_config(
                                cur_key,
                                &name,
                                ERR_INVALID_VAR_NAME,
                            ));
                        }
                    }
                    new_condition.expected = vars;
                }
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
            "script",
            "tasks",
            "recurring",
            "max_tasks_retries",
            "execute_sequence",
            "break_on_failure",
            "break_on_success",
            "suspended",
            "expect_all",
            "recur_after_failed_check",
            "expected_results",
            "check_after",
        ];
        cfg_check_keys(cfgmap, &check)?;

        // common mandatory parameter check

        // type and name are both mandatory: type is checked and name is kept
        cfg_mandatory!(cfg_string_check_exact(cfgmap, "type", "lua"))?;
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

        // check configuration task list against the provided ones
        if let Some(v) = cfg_vec_string_check_regex(cfgmap, "tasks", &RE_TASK_NAME)? {
            for s in v {
                if !available_tasks.contains(&s.as_str()) {
                    return Err(cfg_err_invalid_config(
                        cur_key,
                        &s,
                        ERR_INVALID_TASK,
                    ));
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

        cfg_bool(cfgmap, "expect_all")?;
        cfg_bool(cfgmap, "recur_after_failed_check")?;

        // expected results are in a complex map, thus no shortcut is given
        let cur_key = "expected_results";
        if cfgmap.contains_key(cur_key) {
            if let Some(item) = cfgmap.get(cur_key) {
                if !item.is_map() {
                    return Err(cfg_err_invalid_config(
                        cur_key,
                        STR_UNKNOWN_VALUE,
                        ERR_INVALID_PARAMETER,
                    ));
                } else {
                    let map = item.as_map().unwrap();
                    for name in map.keys() {
                        if !RE_VAR_NAME.is_match(name) {
                            return Err(cfg_err_invalid_config(
                                cur_key,
                                &name,
                                ERR_INVALID_VAR_NAME,
                            ));
                        } else if let Some(value) = map.get(name) {
                            if !(value.is_bool() || value.is_int() || value.is_float() || value.is_str()) {
                                return Err(cfg_err_invalid_config(
                                    cur_key,
                                    STR_UNKNOWN_VALUE,
                                    ERR_INVALID_VAR_VALUE,
                                ));
                            }
                        } else {
                            return Err(cfg_err_invalid_config(
                                cur_key,
                                &name,
                                ERR_INVALID_VAR_NAME,
                            ));
                        }
                    }
                }
            }
        }

        Ok(name)
    }

}



impl Condition for LuaCondition {

    fn set_id(&mut self, id: i64) { self.cond_id = id; }
    fn get_name(&self) -> String { self.cond_name.clone() }
    fn get_id(&self) -> i64 { self.cond_id }
    fn get_type(&self) -> &str { "lua" }

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


    fn suspended(&self) -> bool { self.suspended }
    fn recurring(&self) -> bool { self.recurring }
    fn has_succeeded(&self) -> bool { self.has_succeeded }

    fn exec_sequence(&self) -> bool { self.exec_sequence }
    fn break_on_success(&self) -> bool { self.break_on_success }
    fn break_on_failure(&self) -> bool { self.break_on_failure }

    fn last_checked(&self) -> Option<Instant> { self.last_tested }
    fn last_succeeded(&self) -> Option<Instant> { self.last_succeeded }
    fn startup_time(&self) -> Option<Instant> { self.startup_time }

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
            self.task_names.remove(
                self.task_names
                .iter()
                .position(|x| x == &name)
                .unwrap());
            Ok(true)
        } else {
            Ok(false)
        }
    }


    /// Mandatory check function.
    ///
    /// This function actually performs the test: if the underlying OS command
    /// exits and the success criteria are met, the condition is verified.
    ///
    /// **NOTE**: this is an _almost exact_ copy of the `_run()` method in
    ///           the _Lua_ script based `LuaTask` task structure.
    fn _check_condition(&mut self) -> Result<Option<bool>> {
        self.log(
            LogType::Debug,
            LOG_WHEN_START,
            LOG_STATUS_MSG,
            "checking Lua script based condition",
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

        let mut failure_reason = FailureReason::NoCheck;

        fn inner_log(id: i64, name: &str, severity: LogType, message: &str) {
            log(
                severity,
                LOG_EMITTER_CONDITION,
                LOG_ACTION_LUA,
                Some((name, id)),
                LOG_WHEN_PROC,
                LOG_STATUS_MSG,
                message,
            );
        }

        let lua = mlua::Lua::new_with(mlua::StdLib::ALL_SAFE, mlua::LuaOptions::new());
        if lua.is_err() {
            let e = lua.unwrap_err();
            self.log(
                LogType::Debug,
                LOG_WHEN_START,
                LOG_STATUS_FAIL,
                &format!(
                    "cannot start Lua interpreter ({})",
                    e.to_string(),
                ),
            );
            return Err(Error::new(
                Kind::Failed,
                &format!("cannot start Lua interpreter ({})", e.to_string()),
            ));
        }
        let lua = lua.unwrap();

        self.log(
            LogType::Trace,
            LOG_WHEN_START,
            LOG_STATUS_MSG,
            "executing Lua script for condition check",
        );

        // start execution
        let startup_time = SystemTime::now();

        let globals = lua.globals();

        // set Lua variables if configured to do so
        if self.set_vars {
            let _ = globals.set(LUAVAR_NAME_COND.as_str(), self.cond_name.to_string());
        }

        // create functions for logging in a table called `log`
        let logftab = lua.create_table().unwrap();

        let id = self.get_id();
        let name = self.get_name();
        let _ = logftab.set("debug", lua.create_function(move
            |_, s: String| Ok(inner_log(id, &name, LogType::Debug, &s)))
            .unwrap());

        let id = self.get_id();
        let name = self.get_name();
        let _ = logftab.set("trace", lua.create_function(move
            |_, s: String| Ok(inner_log(id, &name, LogType::Trace, &s)))
            .unwrap());

        let id = self.get_id();
        let name = self.get_name();
        let _ = logftab.set("info", lua.create_function(move
            |_, s: String| Ok(inner_log(id, &name, LogType::Info, &s)))
            .unwrap());

        let id = self.get_id();
        let name = self.get_name();
        let _ = logftab.set("warn", lua.create_function(move
            |_, s: String| Ok(inner_log(id, &name, LogType::Warn, &s)))
            .unwrap());

        let id = self.get_id();
        let name = self.get_name();
        let _ = logftab.set("error", lua.create_function(move
            |_, s: String| Ok(inner_log(id, &name, LogType::Error, &s)))
            .unwrap());

        let _ = globals.set("log", logftab);

        match lua.load(&self.script.clone()).exec() {
            // if the script executed without error, iterate over the provided
            // names and values to check that the results match expectations;
            // obviously if no varnames/values are provided, no iteration will
            // occur and the outcome remains `FailureReason::NoCheck`.
            Ok(()) => {
                // if all values are to be checked: assume no error initially,
                // break at first mismatch, set `FailureReason::VariableMatch`;
                // otherwise: assume error initially, break at first match, and
                // set `FailureReason::NoFailure`
                if !self.expected.is_empty() {
                    self.log(
                        LogType::Debug,
                        LOG_WHEN_PROC,
                        LOG_STATUS_MSG,
                        &format!("checking results: {}", &self.repr_checks()),
                    );
                    if self.expect_all {
                        failure_reason = FailureReason::NoFailure;
                        for (varname, value) in self.expected.iter() {
                            if let Some(res) = match value {
                                LuaValue::LuaString(v) => {
                                    let r: std::result::Result<String, mlua::Error> = globals.get(varname.as_str());
                                    if let Ok(r) = r {
                                        Some(r == *v)
                                    } else { None }
                                }
                                LuaValue::LuaNumber(v) => {
                                    let r: std::result::Result<f64, mlua::Error> = globals.get(varname.as_str());
                                    if let Ok(r) = r {
                                        Some(r == *v)
                                    } else { None }
                                }
                                LuaValue::LuaBoolean(v) => {
                                    let r: std::result::Result<bool, mlua::Error> = globals.get(varname.as_str());
                                    if let Ok(r) = r {
                                        Some(r == *v)
                                    } else { None }
                                }
                            } {
                                if !res {
                                    self.log(
                                        LogType::Debug,
                                        LOG_WHEN_END,
                                        LOG_STATUS_MSG,
                                        &format!("result mismatch on at least one variable ({varname}): failure"),
                                    );
                                    failure_reason = FailureReason::VariableMatch;
                                    break;
                                }
                            } else {
                                self.log(
                                    LogType::Debug,
                                    LOG_WHEN_END,
                                    LOG_STATUS_MSG,
                                    &format!("result not found for at least one variable ({varname}): failure"),
                                );
                                failure_reason = FailureReason::VariableMatch;
                                break;
                            }
                        }
                    } else {
                        failure_reason = FailureReason::VariableMatch;
                        for (varname, value) in self.expected.iter() {
                            if let Some(res) = match value {
                                LuaValue::LuaString(v) => {
                                    let r: std::result::Result<String, mlua::Error> = globals.get(varname.as_str());
                                    if let Ok(r) = r {
                                        Some(r == *v)
                                    } else { None }
                                }
                                LuaValue::LuaNumber(v) => {
                                    let r: std::result::Result<f64, mlua::Error> = globals.get(varname.as_str());
                                    if let Ok(r) = r {
                                        Some(r == *v)
                                    } else { None }
                                }
                                LuaValue::LuaBoolean(v) => {
                                    let r: std::result::Result<bool, mlua::Error> = globals.get(varname.as_str());
                                    if let Ok(r) = r {
                                        Some(r == *v)
                                    } else { None }
                                }
                            } {
                                if res {
                                    self.log(
                                        LogType::Debug,
                                        LOG_WHEN_END,
                                        LOG_STATUS_MSG,
                                        &format!("result match on at least one variable ({varname}): success"),
                                    );
                                    failure_reason = FailureReason::NoFailure;
                                    break;
                                }
                            }
                        }
                    }
                }
            }

            // in case of error report a brief error message to the log
            Err(res) => {
                if let Some(err_msg) = res.to_string().split('\n').next() {
                    self.log(
                        LogType::Warn,
                        LOG_WHEN_END,
                        LOG_STATUS_FAIL,
                        &format!("error in Lua script: {}", err_msg),
                    );
                } else {
                    self.log(
                        LogType::Warn,
                        LOG_WHEN_END,
                        LOG_STATUS_FAIL,
                        "error in Lua script (unknown)",
                    );
                }
                failure_reason = FailureReason::ScriptError;
            }
        }

        // log the final message and return the condition outcome
        let duration = SystemTime::now().duration_since(startup_time).unwrap();
        match failure_reason {
            FailureReason::NoFailure => {
                let succeeds = self.last_check_failed || !self.recur_after_failed_check;
                self.last_check_failed = false;
                self.log(
                    LogType::Debug,
                    LOG_WHEN_END,
                    LOG_STATUS_OK,
                    &format!(
                        "condition checked successfully in {:.2}s",
                        duration.as_secs_f64(),
                    ),
                );
                if succeeds {
                    Ok(Some(true))
                } else {
                    self.log(
                        LogType::Debug,
                        LOG_WHEN_END,
                        LOG_STATUS_MSG,
                        &format!(
                            "persistent success status: waiting for failure to recur",
                        ),
                    );
                    Ok(Some(false))
                }
            }
            FailureReason::NoCheck => {
                self.last_check_failed = true;
                self.log(
                    LogType::Debug,
                    LOG_WHEN_END,
                    LOG_STATUS_FAIL,
                    &format!(
                        "condition checked with no outcome in {:.2}s",
                        duration.as_secs_f64(),
                    ),
                );
                Ok(None)
            }
            FailureReason::VariableMatch => {
                self.last_check_failed = true;
                self.log(
                    LogType::Debug,
                    LOG_WHEN_END,
                    LOG_STATUS_FAIL,
                    &format!(
                        "condition checked unsuccessfully (unmatched values) in {:.2}s",
                        duration.as_secs_f64(),
                    ),
                );
                Ok(Some(false))
            }
            FailureReason::ScriptError => {
                self.last_check_failed = true;
                self.log(
                    LogType::Warn,
                    LOG_WHEN_END,
                    LOG_STATUS_FAIL,
                    &format!(
                        "condition checked unsuccessfully (script error) in {:.2}s",
                        duration.as_secs_f64(),
                    ),
                );
                Ok(Some(false))
            }
        }

    }

}


// end.
