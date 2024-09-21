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
use rlua;

use super::base::Condition;
use crate::task::registry::TaskRegistry;
use crate::common::logging::{log, LogType};
use crate::constants::*;


/// The possible values to be checked from Lua
enum LuaValue {
    LuaString(String),
    LuaNumber(f64),
    LuaBoolean(bool),
}


/// In case of failure, the reason will be one of the provided values
#[derive(Debug, PartialEq)]
pub enum FailureReason {
    NoCheck,
    NoFailure,
    VariableMatch,
    ScriptError,
}



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

    // specific members
    // parameters
    script: String,
    set_vars: bool,
    expected: HashMap<String, LuaValue>,
    expect_all: bool,
    check_after: Option<Duration>,

    // internal values
    check_last: Instant,
}


// implement the hash protocol
impl Hash for LuaCondition {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // common part
        self.cond_name.hash(state);
        self.recurring.hash(state);
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

            // specific members initialization
            // parameters
            script: String::from(script),
            set_vars: true,
            expected: HashMap::new(),
            expect_all: false,
            check_after: None,

            // internal values
            check_last: t,
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
    ///
    /// The TOML configuration file format is the following
    ///
    /// ```toml
    /// # definition (mandatory)
    /// [[condition]]
    /// name = "CommandConditionName"
    /// name = "LuaTaskName"
    /// type = "lua"                                # mandatory value
    /// script = '''
    ///     log.info("hello from Lua");
    ///     result = 10;
    ///     '''
    ///
    /// # optional parameters (if omitted, defaults are used)
    /// expect_all = false
    /// expected_results = { result = 10, ... }
    /// tasks = [ "Task1", "Task2", ... ]
    /// ```
    ///
    /// Note that the script must be inline in the TOML file: this means that
    /// the value for the `script` parameter cannot be the path to a script.
    /// However the script can contain the `require` function, or directly
    /// invoke a script via `dofile("/path/to/script.lua")` in a one-liner.
    ///
    /// Any incorrect value will cause an error. The value of the `type` entry
    /// *must* be set to `"lua"` mandatorily for this type of `Condition`.
    pub fn load_cfgmap(cfgmap: &CfgMap, task_registry: &'static TaskRegistry) -> std::io::Result<LuaCondition> {

        fn _invalid_cfg(key: &str, value: &str, message: &str) -> std::io::Result<LuaCondition> {
            Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("{ERR_INVALID_COND_CONFIG}: ({key}={value}) {message}"),
            ))
        }

        let check = [
            "type",
            "name",
            "tags",
            "script",
            "tasks",
            "recurring",
            "execute_sequence",
            "break_on_failure",
            "break_on_success",
            "suspended",
            "expect_all",
            "expected_results",
            "check_after",
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
                    ERR_INVALID_COND_TYPE);
            }
            cond_type = item.as_str().unwrap().to_owned();
            if cond_type != "lua" {
                return _invalid_cfg(cur_key,
                    &cond_type,
                    ERR_INVALID_COND_TYPE);
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
                    ERR_INVALID_COND_NAME);
            }
            name = item.as_str().unwrap().to_owned();
            if !RE_COND_NAME.is_match(&name) {
                return _invalid_cfg(cur_key,
                    &name,
                    ERR_INVALID_COND_NAME);
            }
        } else {
            return _invalid_cfg(
                cur_key,
                STR_UNKNOWN_VALUE,
                ERR_MISSING_PARAMETER);
        }

        // common mandatory parameter retrieval
        let cur_key = "script";
        let script;
        if let Some(item) = cfgmap.get(cur_key) {
            if !item.is_str() {
                return _invalid_cfg(
                    cur_key,
                    STR_UNKNOWN_VALUE,
                    ERR_INVALID_PARAMETER);
            }
            script = item.as_str().unwrap().to_owned();
        } else {
            return _invalid_cfg(cur_key,
                STR_UNKNOWN_VALUE,
                ERR_MISSING_PARAMETER);
        }

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
        let cur_key = "tags";
        if let Some(item) = cfgmap.get(cur_key) {
            if !item.is_list() && !item.is_map() {
                return _invalid_cfg(
                    cur_key,
                    STR_UNKNOWN_VALUE,
                    ERR_INVALID_PARAMETER);
            }
        }

        let cur_key = "tasks";
        if let Some(item) = cfgmap.get(cur_key) {
            if !item.is_list() {
                return _invalid_cfg(
                    cur_key,
                    STR_UNKNOWN_VALUE,
                    ERR_INVALID_TASK_LIST);
            }
            for a in item.as_list().unwrap() {
                let s = String::from(a.as_str().unwrap_or(&String::new()));
                if !new_condition.add_task(&s)? {
                    return _invalid_cfg(
                        cur_key,
                        &s,
                        ERR_INVALID_TASK);
                }
            }
        }

        let cur_key = "recurring";
        if let Some(item) = cfgmap.get(cur_key) {
            if !item.is_bool() {
                return _invalid_cfg(
                    cur_key,
                    STR_UNKNOWN_VALUE,
                    ERR_INVALID_PARAMETER);
            } else {
                new_condition.recurring = *item.as_bool().unwrap();
            }
        }

        let cur_key = "execute_sequence";
        if let Some(item) = cfgmap.get(cur_key) {
            if !item.is_bool() {
                return _invalid_cfg(
                    cur_key,
                    STR_UNKNOWN_VALUE,
                    ERR_INVALID_PARAMETER);
            } else {
                new_condition.exec_sequence = *item.as_bool().unwrap();
            }
        }

        let cur_key = "break_on_failure";
        if let Some(item) = cfgmap.get(cur_key) {
            if !item.is_bool() {
                return _invalid_cfg(
                    cur_key,
                    STR_UNKNOWN_VALUE,
                    ERR_INVALID_PARAMETER);
            } else {
                new_condition.break_on_failure = *item.as_bool().unwrap();
            }
        }

        let cur_key = "break_on_success";
        if let Some(item) = cfgmap.get(cur_key) {
            if !item.is_bool() {
                return _invalid_cfg(
                    cur_key,
                    STR_UNKNOWN_VALUE,
                    ERR_INVALID_PARAMETER);
            } else {
                new_condition.break_on_success = *item.as_bool().unwrap();
            }
        }

        let cur_key = "suspended";
        if let Some(item) = cfgmap.get(cur_key) {
            if !item.is_bool() {
                return _invalid_cfg(
                    cur_key,
                    STR_UNKNOWN_VALUE,
                    ERR_INVALID_PARAMETER);
            } else {
                new_condition.suspended = *item.as_bool().unwrap();
            }
        }

        let cur_key = "check_after";
        if let Some(item) = cfgmap.get(cur_key) {
            if !item.is_int() {
                return _invalid_cfg(
                    cur_key,
                    STR_UNKNOWN_VALUE,
                    ERR_INVALID_PARAMETER);
            } else {
                let i = *item.as_int().unwrap();
                if i < 1 {
                    return _invalid_cfg(
                        cur_key,
                        &i.to_string(),
                        ERR_INVALID_PARAMETER);
                }
                new_condition.check_after = Some(Duration::from_secs(i as u64));
            }
        }

        // specific optional parameter initialization
        let cur_key = "expect_all";
        if cfgmap.contains_key(cur_key) {
            if let Some(item) = cfgmap.get(cur_key) {
                if !item.is_bool() {
                    return _invalid_cfg(
                        cur_key,
                        STR_UNKNOWN_VALUE,
                        ERR_INVALID_PARAMETER);
                } else {
                    new_condition.expect_all = *item.as_bool().unwrap();
                }
            }
        }

        let cur_key = "expected_results";
        if cfgmap.contains_key(cur_key) {
            if let Some(item) = cfgmap.get(cur_key) {
                if !item.is_map() {
                    return _invalid_cfg(
                        cur_key,
                        STR_UNKNOWN_VALUE,
                        ERR_INVALID_PARAMETER);
                } else {
                    let map = item.as_map().unwrap();
                    let mut vars: HashMap<String, LuaValue> = HashMap::new();
                    for name in map.keys() {
                        if !RE_VAR_NAME.is_match(name) {
                            return _invalid_cfg(
                                cur_key,
                                &name,
                                ERR_INVALID_VAR_NAME);
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
                                return _invalid_cfg(
                                    cur_key,
                                    STR_UNKNOWN_VALUE,
                                    ERR_INVALID_VAR_VALUE);
                            }
                        } else {
                            return _invalid_cfg(
                                cur_key,
                                &name,
                                ERR_INVALID_VAR_NAME);
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

    fn set_checked(&mut self) -> Result<bool, std::io::Error> {
        self.last_tested = Some(Instant::now());
        Ok(true)
    }

    fn set_succeeded(&mut self) -> Result<bool, std::io::Error> {
        self.last_succeeded = self.last_tested;
        self.has_succeeded = true;
        Ok(true)
    }

    fn reset_succeeded(&mut self) -> Result<bool, std::io::Error> {
        self.last_succeeded = None;
        self.has_succeeded = false;
        Ok(true)
    }

    fn reset(&mut self) -> Result<bool, std::io::Error> {
        self.last_tested = None;
        self.last_succeeded = None;
        self.has_succeeded = false;
        Ok(true)
    }


    fn start(&mut self) -> Result<bool, std::io::Error> {
        self.suspended = false;
        self.startup_time = Some(Instant::now());
        Ok(true)
    }

    fn suspend(&mut self) -> Result<bool, std::io::Error> {
        if self.suspended {
            Ok(false)
        } else {
            self.suspended = true;
            Ok(true)
        }
    }

    fn resume(&mut self) -> Result<bool, std::io::Error> {
        if self.suspended {
            self.suspended = false;
            Ok(true)
        } else {
            Ok(false)
        }
    }


    fn task_names(&self) -> Result<Vec<String>, std::io::Error> {
        Ok(self.task_names.clone())
    }


    fn _add_task(&mut self, name: &str) -> Result<bool, std::io::Error> {
        let name = String::from(name);
        if self.task_names.contains(&name) {
            Ok(false)
        } else {
            self.task_names.push(name);
            Ok(true)
        }
    }

    fn _remove_task(&mut self, name: &str) -> Result<bool, std::io::Error> {
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
    fn _check_condition(&mut self) -> Result<Option<bool>, std::io::Error> {
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

        self.log(
            LogType::Debug,
            LOG_WHEN_START,
            LOG_STATUS_MSG,
            "executing Lua script for condition check",
        );

        // start execution
        let startup_time = SystemTime::now();

        let lua = rlua::Lua::new_with(rlua::StdLib::ALL_NO_DEBUG);
        lua.context(|lctx| {

            let globals = lctx.globals();

            // set Lua variables if configured to do so
            if self.set_vars {
                let _ = globals.set::<&str, String>(LUAVAR_NAME_COND.as_ref(), self.cond_name.to_string());
            }

            // create functions for logging in a table called `log`
            let logftab = lctx.create_table().unwrap();

            let id = self.get_id();
            let name = self.get_name();
            let _ = logftab.set("debug", lctx.create_function(move
                |_, s: String| Ok(inner_log(id, &name, LogType::Debug, &s)))
                .unwrap());

            let id = self.get_id();
            let name = self.get_name();
            let _ = logftab.set("trace", lctx.create_function(move
                |_, s: String| Ok(inner_log(id, &name, LogType::Trace, &s)))
                .unwrap());

            let id = self.get_id();
            let name = self.get_name();
            let _ = logftab.set("info", lctx.create_function(move
                |_, s: String| Ok(inner_log(id, &name, LogType::Info, &s)))
                .unwrap());

            let id = self.get_id();
            let name = self.get_name();
            let _ = logftab.set("warn", lctx.create_function(move
                |_, s: String| Ok(inner_log(id, &name, LogType::Warn, &s)))
                .unwrap());

            let id = self.get_id();
            let name = self.get_name();
            let _ = logftab.set("error", lctx.create_function(move
                |_, s: String| Ok(inner_log(id, &name, LogType::Error, &s)))
                .unwrap());

            let _ = globals.set("log", logftab);

            match lctx.load(&self.script.clone()).exec() {
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
                                if let Some(res) = lua.context(|lctx| {
                                    let globals = lctx.globals();
                                    match value {
                                        LuaValue::LuaString(v) => {
                                            if let Ok(r) = globals.get::<_, String>(varname.clone()) {
                                                Some(r == *v)
                                            } else { None }
                                        }
                                        LuaValue::LuaNumber(v) => {
                                            if let Ok(r) = globals.get::<_, f64>(varname.clone()) {
                                                Some(r == *v)
                                            } else { None }
                                        }
                                        LuaValue::LuaBoolean(v) => {
                                            if let Ok(r) = globals.get::<_, bool>(varname.clone()) {
                                                Some(r == *v)
                                            } else { None }
                                        }
                                    }
                                }) {
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
                                if let Some(res) = lua.context(|lctx| {
                                    let globals = lctx.globals();
                                    match value {
                                        LuaValue::LuaString(v) => {
                                            if let Ok(r) = globals.get::<_, String>(varname.clone()) {
                                                Some(r == *v)
                                            } else { None }
                                        }
                                        LuaValue::LuaNumber(v) => {
                                            if let Ok(r) = globals.get::<_, f64>(varname.clone()) {
                                                Some(r == *v)
                                            } else { None }
                                        }
                                        LuaValue::LuaBoolean(v) => {
                                            if let Ok(r) = globals.get::<_, bool>(varname.clone()) {
                                                Some(r == *v)
                                            } else { None }
                                        }
                                    }
                                }) {
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

        });

        // log the final message and return the condition outcome
        let duration = SystemTime::now().duration_since(startup_time).unwrap();
        match failure_reason {
            FailureReason::NoFailure => {
                self.log(
                    LogType::Info,
                    LOG_WHEN_END,
                    LOG_STATUS_OK,
                    &format!(
                        "condition checked successfully in {:.2}s",
                        duration.as_secs_f64()));
                Ok(Some(true))
            }
            FailureReason::NoCheck => {
                self.log(
                    LogType::Info,
                    LOG_WHEN_END,
                    LOG_STATUS_FAIL,
                    &format!(
                        "condition checked with no outcome in {:.2}s",
                        duration.as_secs_f64()));
                Ok(None)
            }
            FailureReason::VariableMatch => {
                self.log(
                    LogType::Info,
                    LOG_WHEN_END,
                    LOG_STATUS_FAIL,
                    &format!(
                        "condition checked unsuccessfully (unmatched values) in {:.2}s",
                        duration.as_secs_f64()));
                Ok(Some(false))
            }
            FailureReason::ScriptError => {
                self.log(
                    LogType::Info,
                    LOG_WHEN_END,
                    LOG_STATUS_FAIL,
                    &format!(
                        "condition checked unsuccessfully (script error) in {:.2}s",
                        duration.as_secs_f64()));
                Ok(Some(false))
            }
        }

    }

}


// end.
