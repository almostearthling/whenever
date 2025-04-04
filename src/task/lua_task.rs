//! Define a _Lua_ script based command
//!
//! This `Task` executes a _Lua_ script provided in the configuration, then
//! checks the contents of one or more variables to match the expected result
//! and reports success or failure accordingly. An error in the script always
//! results as a failure.



use std::collections::HashMap;
use std::time::SystemTime;
use std::hash::{DefaultHasher, Hash, Hasher};

use itertools::Itertools;

use cfgmap::CfgMap;
use mlua;


// we implement the Task trait here in order to enqueue tasks
use super::base::Task;
use crate::common::logging::{log, LogType};
use crate::common::wres::{Error, Kind, Result};
use crate::common::luaitem::*;
use crate::{cfg_mandatory, constants::*};

use crate::cfghelp::*;



/// _Lua_ script Based Task
///
/// This type of task runs a _Lua_ script and possibly matches one or more
/// variables against the provided expected values.
pub struct LuaTask {
    // common members
    task_id: i64,
    task_name: String,

    // specific members
    // parameters
    script: String,
    set_vars: bool,
    expected: HashMap<String, LuaValue>,
    expect_all: bool,
}


// implement the hash protocol
impl Hash for LuaTask {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.task_name.hash(state);
        self.script.hash(state);
        self.set_vars.hash(state);
        self.expect_all.hash(state);

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
impl LuaTask {

    /// Create a new _Lua_ script based task
    ///
    /// The only parameters that have to be set mandatorily upon creation of a
    /// _Lua_ script based task are the following.
    ///
    /// # Arguments
    ///
    /// * `name` - a string containing the name of the task
    /// * `script` - a working Lua script consisting of at least one chunk
    ///
    /// By default it is not necessary to provide checks (that is, variable
    /// names to test for vaules) and the result of the script will be
    /// indefinite (`Ok(None)`), indicating that no test has been performed.
    /// The variable names and values to check are provided via constructor
    /// modifiers. Checks are supported for string, number, and boolean
    /// values.
    ///
    /// If variable names and values to check against are provided, then the
    /// test is performed and either `Ok(Some(true))` or `Ok(Some(false))`
    /// respectively denoting success or failure are returned.
    ///
    /// Errors in the script will _always_ be considered a failure.
    pub fn new(
        name: &str,
        script: &str,
    ) -> Self {
        log(
            LogType::Debug,
            LOG_EMITTER_TASK_LUA,
            LOG_ACTION_NEW,
            Some((name, 0)),
            LOG_WHEN_INIT,
            LOG_STATUS_MSG,
            &format!("TASK {name}: creating a new Lua script based task")
        );
        LuaTask {
            task_id: 0,
            task_name: String::from(name),

            // specific members initialization
            script: String::from(script),
            set_vars: true,
            expected: HashMap::new(),
            expect_all: false,
        }
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


    /// Constructor modifier to specify that the task should not set the
    /// context variables that specify the task name and the condition that
    /// triggered the task, when set to `false`. The default behaviour is to
    /// export those variables.
    pub fn sets_vars(mut self, yes: bool) -> Self {
        self.set_vars = yes;
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

    /// Load a `LuaTask` from a [`CfgMap`](https://docs.rs/cfgmap/latest/)
    ///
    /// The `LuaTask` is initialized according to the values provided in
    /// the `CfgMap` argument. If the `CfgMap` format does not comply with
    /// the requirements of a `LuaTask` an error is raised.
    pub fn load_cfgmap(cfgmap: &CfgMap) -> Result<LuaTask> {

        let check = vec![
            "type",
            "name",
            "tags",
            "script",
            "expect_all",
            "expected_results",
        ];
        cfg_check_keys(cfgmap, &check)?;

        // type and name are both mandatory but type is only checked
        cfg_mandatory!(cfg_string_check_exact(cfgmap, "type", "lua"))?;
        let name = cfg_mandatory!(cfg_string_check_regex(cfgmap, "name", &RE_TASK_NAME))?.unwrap();

        // specific mandatory parameter retrieval
        let script = cfg_mandatory!(cfg_string(cfgmap, "script"))?.unwrap();

        // initialize the structure
        let mut new_task = LuaTask::new(
            &name,
            &script,
        );

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

        // specific optional parameter initialization
        if let Some(v) = cfg_bool(cfgmap, "expect_all")? {
            new_task.expect_all = v;
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
                    new_task.expected = vars;
                }
            }
        }

        Ok(new_task)
    }

    /// Check a configuration map and return item name if Ok
    ///
    /// The check is performed exactly in the same way and in the same order
    /// as in `load_cfgmap`, the only difference is that no actual item is
    /// created and that a name is returned, which is the name of the item that
    /// _would_ be created via the equivalent call to `load_cfgmap`
    pub fn check_cfgmap(cfgmap: &CfgMap) -> Result<String> {

        let check = vec![
            "type",
            "name",
            "tags",
            "script",
            "expect_all",
            "expected_results",
        ];
        cfg_check_keys(cfgmap, &check)?;

        // common mandatory parameter check

        // type and name are both mandatory: type is checked and name is kept
        cfg_mandatory!(cfg_string_check_exact(cfgmap, "type", "lua"))?;
        let name = cfg_mandatory!(cfg_string_check_regex(cfgmap, "name", &RE_TASK_NAME))?.unwrap();

        // specific mandatory parameter check
        cfg_mandatory!(cfg_string(cfgmap, "script"))?.unwrap();

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

        cfg_bool(cfgmap, "expect_all")?;

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
                }
            }
        }

        Ok(name)
    }

}



// implement the Task trait
impl Task for LuaTask {

    fn set_id(&mut self, id: i64) { self.task_id = id; }
    fn get_name(&self) -> String { self.task_name.clone() }
    fn get_id(&self) -> i64 { self.task_id }


    /// Return a hash of this item for comparison
    fn _hash(&self) -> u64 {
        let mut s = DefaultHasher::new();
        self.hash(&mut s);
        s.finish()
    }

    /// Execute this `LuaTask`
    ///
    /// This implementation of the trait `run()` function obeys to the main
    /// trait's constraints, and returns
    ///
    /// * `Ok(Some(true))` on success
    /// * `Ok(Some(false))` on check failure or script error
    /// * `Ok(None)` when the script didn't check for result
    /// * `Err(_)` never
    ///
    /// The interpreter loads the whole standard library prior to execution
    /// of the script. Moreover a `log` table is provided containing the
    /// following functions:
    ///
    /// * `debug`
    /// * `trace`
    /// * `info`
    /// * `warn`
    /// * `error`
    ///
    /// that can be used to directly log from the _Lua_ script. Note that
    /// the resulting log will anyway comply to the application format, that
    /// means for example that it will be prefixed with the context. All of
    /// these functions take a _Lua_ string as input and write it to the log
    /// with appropriate severity: in case a certain severity level is
    /// configured for the log, only messages above that severity level are
    /// logged.
    fn _run(
        &mut self,
        trigger_name: &str,
    ) -> Result<Option<bool>> {
        let mut failure_reason = FailureReason::NoCheck;

        fn inner_log(trigger_name: &str, id: i64, name: &str, severity: LogType, message: &str) {
            log(
                severity,
                "TASK",
                LOG_ACTION_LUA,
                Some((name, id)),
                LOG_WHEN_PROC,
                LOG_STATUS_MSG,
                &format!("(trigger: {trigger_name}) (Lua) {message}"),
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
                    "(trigger: {trigger_name}) cannot start Lua interpreter ({})",
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
            LogType::Debug,
            LOG_WHEN_START,
            LOG_STATUS_OK,
            &format!("(trigger: {trigger_name}) executing Lua script as a task"),
        );

        // start execution
        let startup_time = SystemTime::now();

        let globals = lua.globals();

        // set Lua variables if configured to do so
        if self.set_vars {
            let _ = globals.set(LUAVAR_NAME_COND.as_str(), trigger_name.to_string());
            let _ = globals.set(LUAVAR_NAME_TASK.as_str(), self.task_name.to_string());
        }

        // create functions for logging in a table called `log`
        let logftab = lua.create_table().unwrap();

        let id = self.get_id();
        let name = self.get_name();
        let trigger = String::from(trigger_name);
        let _ = logftab.set("debug", lua.create_function(move
            |_, s: String| Ok(inner_log(&trigger, id, &name, LogType::Debug, &s)))
            .unwrap());

        let id = self.get_id();
        let name = self.get_name();
        let trigger = String::from(trigger_name);
        let _ = logftab.set("trace", lua.create_function(move
            |_, s: String| Ok(inner_log(&trigger, id, &name, LogType::Trace, &s)))
            .unwrap());

        let id = self.get_id();
        let name = self.get_name();
        let trigger = String::from(trigger_name);
        let _ = logftab.set("info", lua.create_function(move
            |_, s: String| Ok(inner_log(&trigger, id, &name, LogType::Info, &s)))
            .unwrap());

        let id = self.get_id();
        let name = self.get_name();
        let trigger = String::from(trigger_name);
        let _ = logftab.set("warn", lua.create_function(move
            |_, s: String| Ok(inner_log(&trigger, id, &name, LogType::Warn, &s)))
            .unwrap());

        let id = self.get_id();
        let name = self.get_name();
        let trigger = String::from(trigger_name);
        let _ = logftab.set("error", lua.create_function(move
            |_, s: String| Ok(inner_log(&trigger, id, &name, LogType::Error, &s)))
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
                        &format!(
                            "(trigger: {trigger_name}) checking results: {}",
                            &self.repr_checks(),
                        ),
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
                                        LOG_WHEN_PROC,
                                        LOG_STATUS_OK,
                                        &format!("(trigger: {trigger_name}) result mismatch on at least one variable ({varname}): failure"),
                                    );
                                    failure_reason = FailureReason::VariableMatch;
                                    break;
                                }
                            } else {
                                self.log(
                                    LogType::Debug,
                                    LOG_WHEN_PROC,
                                    LOG_STATUS_FAIL,
                                    &format!("(trigger: {trigger_name}) result not found for at least one variable ({varname}): failure"),
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
                                        LOG_WHEN_PROC,
                                        LOG_STATUS_OK,
                                        &format!("(trigger: {trigger_name}) result match on at least one variable ({varname}): success"),
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
                        &format!("error in Lua script: {err_msg}"),
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
                self.log(
                    LogType::Debug,
                    LOG_WHEN_END,
                    LOG_STATUS_OK,
                    &format!(
                        "(trigger: {trigger_name}) task exited successfully in {:.2}s",
                        duration.as_secs_f64()));
                Ok(Some(true))
            }
            FailureReason::NoCheck => {
                self.log(
                    LogType::Debug,
                    LOG_WHEN_END,
                    LOG_STATUS_OK,
                    &format!(
                        "(trigger: {trigger_name}) task exited with no outcome in {:.2}s",
                        duration.as_secs_f64()));
                Ok(None)
            }
            FailureReason::VariableMatch => {
                self.log(
                    LogType::Debug,
                    LOG_WHEN_END,
                    LOG_STATUS_OK,
                    &format!(
                        "(trigger: {trigger_name}) task exited unsuccessfully (unmatched values) in {:.2}s",
                        duration.as_secs_f64()));
                Ok(Some(false))
            }
            FailureReason::ScriptError => {
                self.log(
                    LogType::Warn,
                    LOG_WHEN_END,
                    LOG_STATUS_FAIL,
                    &format!("(trigger: {trigger_name}) task exited unsuccessfully (script error) in {:.2}s",
                        duration.as_secs_f64()));
                Ok(Some(false))
            }
        }
    }
}


// end.
