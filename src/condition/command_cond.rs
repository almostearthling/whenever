//! Define an OS command based condition
//!
//! This type of `Condition` runs an OS command and checks its outcome to give
//! either a positive or negative result. Similarly to a `CommandTask`, there
//! are several options to determine success or failure, according to the exit
//! code and the presence of an expected or unexpected text in either _stdout_
//! or _stderr_. The text to accept or reject, in all cases, can be checked
//! both as a simple string and as a regular expression pattern. In both cases
//! the match can be either case sensitive or insensitive.
//!
//! The environment can be passed to the command as is, or adding further
//! variable. If requested, the condition can set an extra variable set to
//! its name, so that commands or scripts that are aware of being invoked by
//! the application can have further info about the context.


use std::collections::HashMap;
use std::env;
use std::io::{Error, ErrorKind};
use std::time::{Instant, SystemTime, Duration};
use std::path::PathBuf;
use std::ffi::OsString;

use subprocess::{Popen, PopenConfig, Redirection, ExitStatus};

use cfgmap::CfgMap;

use super::base::Condition;
use crate::task::registry::TaskRegistry;
use crate::common::logging::{log, LogType};
use crate::constants::*;



/// In case of failure, the reason will be one of the provided values
#[derive(Debug, PartialEq)]
pub enum FailureReason {
    NoFailure,
    StdOut,
    StdErr,
    Status,
    Other,
}



/// Command Based Condition
///
/// This condition is verified when the underlying command execution outcome
/// meets the criteria given at construction time.
pub struct CommandCondition {
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
    command: PathBuf,
    args: Vec<String>,
    startup_dir: PathBuf,
    match_exact: bool,
    match_regexp: bool,
    case_sensitive: bool,
    include_env: bool,
    set_envvars: bool,
    environment_vars: HashMap<String, String>,
    check_after: Option<Duration>,
    success_stdout: Option<String>,
    success_stderr: Option<String>,
    success_status: Option<u32>,
    failure_stdout: Option<String>,
    failure_stderr: Option<String>,
    failure_status: Option<u32>,

    // internal values
    check_last: Instant,
    _process_stdout: String,
    _process_stderr: String,
    _process_status: u32,
    _process_failed: bool,
    _process_duration: Duration,
}



#[allow(dead_code)]
impl CommandCondition {

    /// Create a new external command based condition with the given parameters
    pub fn new(
        name: &str,
        command: &PathBuf,
        args: &Vec<String>,
        startup_dir: &PathBuf,
    ) -> Self {
        log(LogType::Debug, "COMMAND_CONDITION new",
            &format!("[INIT/MSG] CONDITION {name}: creating a new command based condition"));
        let t = Instant::now();
        CommandCondition {
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
            command: command.clone(),
            args: args.clone(),
            startup_dir: PathBuf::from(startup_dir),
            match_exact: false,
            match_regexp: false,
            case_sensitive: false,
            include_env: true,
            set_envvars: true,
            environment_vars: HashMap::new(),
            check_after: None,
            success_stdout: None,
            success_stderr: None,
            success_status: None,
            failure_stdout: None,
            failure_stderr: None,
            failure_status: None,

            // internal values
            check_last: t,
            _process_stdout: String::new(),
            _process_stderr: String::new(),
            _process_status: 0,
            _process_failed: false,
            _process_duration: Duration::ZERO,
        }
    }


    // build the full command line, only for logging purposes
    fn command_line(&self) -> String {
        let mut s = String::from(self.command.to_string_lossy());
        for v in self.args.clone().into_iter() {
            if v.contains(" ") || v.contains("\t") || v.contains("\n") || v.contains("\r") {
                s = format!("{s} \"{v}\"");
            } else {
                s = format!("{s} {v}");
            }
        }
        s
    }


    // constructor modifiers
    /// Set the command execution to sequence or parallel
    pub fn execs_sequentially(mut self, yes: bool) -> Self {
        self.exec_sequence = yes;
        return self;
    }

    /// If true, *sequential* task execution will break on first success
    pub fn breaks_on_success(mut self, yes: bool) -> Self {
        self.break_on_success = yes;
        return self;
    }

    /// If true, *sequential* task execution will break on first failure
    pub fn breaks_on_failure(mut self, yes: bool) -> Self {
        self.break_on_failure = yes;
        return self;
    }

    /// If true, create a recurring condition
    pub fn repeats(mut self, yes: bool) -> Self {
        self.recurring = yes;
        return self;
    }

    /// State that the first check and possible following tests are to be
    /// performed after a certain amount of time. This option is present in
    /// this type of condition because the test itself can be both time and
    /// resource consuming, and an user may choose to avoid to perform it
    /// at every tick.
    pub fn checks_after(mut self, delta: Duration) -> Self {
        self.check_after = Some(delta);
        return self;
    }

    /// Load a `CommandCondition` from a [`CfgMap`](https://docs.rs/cfgmap/latest/)
    ///
    /// The `CommandCondition` is initialized according to the values provided
    /// in the `CfgMap` argument. If the `CfgMap` format does not comply with
    /// the requirements of a `CommandCondition` an error is raised.
    ///
    /// The TOML configuration file format is the following
    ///
    /// ```toml
    /// # definition (mandatory)
    /// [[condition]]
    /// name = "CommandConditionName"
    /// type = "command"                            # mandatory value
    ///
    /// startup_path = "/some/startup/directory"    # must exist
    /// command = "executable_name"
    /// command_arguments = [
    ///     "arg1",
    ///     "arg2",
    /// #   ...
    ///     ]
    ///
    /// # optional parameters (if omitted, defaults are used)
    /// recurring = false
    /// execute_sequence = true
    /// break_on_failure = false
    /// break_on_success = false
    /// suspended = true
    /// tasks = [ "Task1", "Task2", ... ]
    /// check_after = 10
    ///
    /// match_exact = false
    /// match_regular_expression = false
    /// success_stdout = "expected"
    /// success_stderr = "expected_error"
    /// success_status = 0
    /// failure_stdout = "unexpected"
    /// failure_stderr = "unexpected_error"
    /// failure_status = 2
    ///
    /// case_sensitive = false
    /// include_environment = true
    /// set_envvironment_variables = true
    /// environment_variables = { VARNAME1 = "value1", VARNAME2 = "value2", ... }
    /// ```
    ///
    /// Any incorrect value will cause an error. The value of the `type` entry
    /// *must* be set to `"command"` mandatorily for this type of `Condition`.
    pub fn load_cfgmap(cfgmap: &CfgMap, task_registry: &'static TaskRegistry) -> std::io::Result<CommandCondition> {

        fn _invalid_cfg(key: &str, value: &str, message: &str) -> std::io::Result<CommandCondition> {
            Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("invalid condition configuration: ({key}={value}) {message}"),
            ))
        }

        let check = vec!(
            "type",
            "name",
            "command",
            "command_arguments",
            "startup_path",
            "tasks",
            "recurring",
            "execute_sequence",
            "break_on_failure",
            "break_on_success",
            "suspended",
            "match_exact",
            "match_regular_expression",
            "case_sensitive",
            "include_environment",
            "set_envvironment_variables",
            "environment_variables",
            "check_after",
            "success_stdout",
            "success_stderr",
            "success_status",
            "failure_stdout",
            "failure_stderr",
            "failure_status",
        );
        for key in cfgmap.keys() {
            if !check.contains(&key.as_str()) {
                return _invalid_cfg(key, STR_UNKNOWN_VALUE,
                    &format!("{ERR_INVALID_CFG_ENTRY} ({key})"));
            }
        }

        // check type
        let cur_key = "type";
        let cond_type;
        if let Some(item) = cfgmap.get(&cur_key) {
            if !item.is_str() {
                return _invalid_cfg(
                    &cur_key,
                    STR_UNKNOWN_VALUE,
                    ERR_INVALID_COND_TYPE);
            }
            cond_type = item.as_str().unwrap().to_owned();
            if cond_type != "command" {
                return _invalid_cfg(&cur_key,
                    &cond_type,
                    ERR_INVALID_COND_TYPE);
            }
        } else {
            return _invalid_cfg(
                &cur_key,
                STR_UNKNOWN_VALUE,
                ERR_MISSING_PARAMETER);
        }

        // common mandatory parameter retrieval
        let cur_key = "name";
        let name;
        if let Some(item) = cfgmap.get(&cur_key) {
            if !item.is_str() {
                return _invalid_cfg(
                    &cur_key,
                    STR_UNKNOWN_VALUE,
                    ERR_INVALID_COND_NAME);
            }
            name = item.as_str().unwrap().to_owned();
            if !RE_COND_NAME.is_match(&name) {
                return _invalid_cfg(&cur_key,
                    &name,
                    ERR_INVALID_COND_NAME);
            }
        } else {
            return _invalid_cfg(
                &cur_key,
                STR_UNKNOWN_VALUE,
                ERR_MISSING_PARAMETER);
        }

        // specific mandatory parameter retrieval
        let cur_key = "command";
        let command;
        if let Some(item) = cfgmap.get(&cur_key) {
            if !item.is_str() {
                return _invalid_cfg(
                    &cur_key,
                    STR_UNKNOWN_VALUE,
                    ERR_INVALID_COMMAND_PATH);
            }
            command = PathBuf::from(item.as_str().unwrap().to_owned());
        } else {
            return _invalid_cfg(
                &cur_key,
                STR_UNKNOWN_VALUE,
                ERR_MISSING_PARAMETER);
        }

        let cur_key = "command_arguments";
        let mut args = Vec::new();
        if let Some(item) = cfgmap.get(&cur_key) {
            if !item.is_list() {
                return _invalid_cfg(
                    &cur_key,
                    STR_UNKNOWN_VALUE,
                    ERR_INVALID_COMMAND_ARGUMENTS);
            }
            for a in item.as_list().unwrap() {
                args.push(a.as_str().unwrap().clone());
            }
        } else {
            return _invalid_cfg(
                &cur_key,
                STR_UNKNOWN_VALUE,
                ERR_MISSING_PARAMETER);
        }

        let cur_key = "startup_path";
        let startup_path;
        if let Some(item) = cfgmap.get(&cur_key) {
            if !item.is_str() {
                return _invalid_cfg(
                    &cur_key,
                    STR_UNKNOWN_VALUE,
                    ERR_INVALID_STARTUP_PATH);
            }
            let sp = item.as_str().unwrap().to_owned();
            // NOTE: canonicalized paths wouldn't often work on Windows
            // startup_path = PathBuf::from(&sp).canonicalize().unwrap_or(PathBuf::new());
            startup_path = PathBuf::from(&sp);
            if !startup_path.is_dir() {
                return _invalid_cfg(
                    &cur_key,
                    &sp,
                    ERR_INVALID_STARTUP_PATH);
            };
        } else {
            return _invalid_cfg(
                &cur_key,
                STR_UNKNOWN_VALUE,
                ERR_MISSING_PARAMETER);
        }

        // initialize the structure
        let mut new_condition = CommandCondition::new(
            &name,
            &command,
            &args,
            &startup_path,
        );
        new_condition.task_registry = Some(&task_registry);

        // by default make condition active if loaded from configuration: if
        // the configuration changes this state the condition will not start
        new_condition.suspended = false;

        // common optional parameter initialization
        let cur_key = "tasks";
        if let Some(item) = cfgmap.get(&cur_key) {
            if !item.is_list() {
                return _invalid_cfg(
                    &cur_key,
                    STR_UNKNOWN_VALUE,
                    ERR_INVALID_TASK_LIST);
            }
            for a in item.as_list().unwrap() {
                let s = String::from(a.as_str().unwrap_or(&String::new()));
                if !new_condition.add_task(&s)? {
                    return _invalid_cfg(
                        &cur_key,
                        &item.as_str().unwrap(),
                        ERR_INVALID_TASK);
                }
            }
        }

        let cur_key = "recurring";
        if let Some(item) = cfgmap.get(cur_key) {
            if !item.is_bool() {
                return _invalid_cfg(
                    &cur_key,
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
                    &cur_key,
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
                    &cur_key,
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
                    &cur_key,
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
                    &cur_key,
                    STR_UNKNOWN_VALUE,
                    ERR_INVALID_PARAMETER);
            } else {
                new_condition.suspended = *item.as_bool().unwrap();
            }
        }

        // specific optional parameter initialization
        let cur_key = "match_exact";
        if let Some(item) = cfgmap.get(cur_key) {
            if !item.is_bool() {
                return _invalid_cfg(
                    &cur_key,
                    STR_UNKNOWN_VALUE,
                    ERR_INVALID_PARAMETER);
            } else {
                new_condition.match_exact = *item.as_bool().unwrap();
            }
        }

        let cur_key = "match_regular_expression";
        if let Some(item) = cfgmap.get(cur_key) {
            if !item.is_bool() {
                return _invalid_cfg(
                    &cur_key,
                    STR_UNKNOWN_VALUE,
                    ERR_INVALID_PARAMETER);
            } else {
                new_condition.match_regexp = *item.as_bool().unwrap();
            }
        }

        let cur_key = "case_sensitive";
        if let Some(item) = cfgmap.get(cur_key) {
            if !item.is_bool() {
                return _invalid_cfg(
                    &cur_key,
                    STR_UNKNOWN_VALUE,
                    ERR_INVALID_PARAMETER);
            } else {
                new_condition.case_sensitive = *item.as_bool().unwrap();
            }
        }

        let cur_key = "include_environment";
        if let Some(item) = cfgmap.get(cur_key) {
            if !item.is_bool() {
                return _invalid_cfg(
                    &cur_key,
                    STR_UNKNOWN_VALUE,
                    ERR_INVALID_PARAMETER);
            } else {
                new_condition.include_env = *item.as_bool().unwrap();
            }
        }

        let cur_key = "set_envvironment_variables";
        if let Some(item) = cfgmap.get(cur_key) {
            if !item.is_bool() {
                return _invalid_cfg(
                    &cur_key,
                    STR_UNKNOWN_VALUE,
                    ERR_INVALID_PARAMETER);
            } else {
                new_condition.set_envvars = *item.as_bool().unwrap();
            }
        }

        let cur_key = "environment_variables";
        if let Some(item) = cfgmap.get(cur_key) {
            if !item.is_map() {
                return _invalid_cfg(
                    &cur_key,
                    STR_UNKNOWN_VALUE,
                    ERR_INVALID_PARAMETER);
            } else {
                let map = item.as_map().unwrap();
                let mut vars: HashMap<String, String> = HashMap::new();
                for name in map.keys() {
                    if !RE_ENVVAR_NAME.is_match(name) {
                        return _invalid_cfg(
                            &cur_key,
                            &item.as_str().unwrap(),
                            ERR_INVALID_ENVVAR_NAME);
                    } else {
                        if let Some(value) = map.get(name) {
                            if value.is_str() || value.is_int() || value.is_float() || value.is_datetime() {
                                vars.insert(name.to_string(), value.as_str().unwrap().to_string());
                            } else {
                                return _invalid_cfg(
                                    &cur_key,
                                    STR_UNKNOWN_VALUE,
                                    ERR_INVALID_ENVVAR_VALUE);
                            }
                        } else {
                            return _invalid_cfg(
                                &cur_key,
                                STR_UNKNOWN_VALUE,
                                ERR_INVALID_ENVVAR_NAME);
                        }
                    }
                }
                new_condition.environment_vars = vars;
            }
        }

        let cur_key = "check_after";
        if let Some(item) = cfgmap.get(cur_key) {
            if !item.is_int() {
                return _invalid_cfg(
                    &cur_key,
                    STR_UNKNOWN_VALUE,
                    ERR_INVALID_PARAMETER);
            } else {
                let i = *item.as_int().unwrap();
                if i < 1 {
                    return _invalid_cfg(
                        &cur_key,
                        &item.as_str().unwrap(),
                        ERR_INVALID_PARAMETER);
                }
                new_condition.check_after = Some(Duration::from_secs(i as u64));
            }
        }

        let cur_key = "success_stdout";
        if let Some(item) = cfgmap.get(&cur_key) {
            if !item.is_str() {
                return _invalid_cfg(
                    &cur_key,
                    STR_UNKNOWN_VALUE,
                    ERR_INVALID_PARAMETER);
            }
            new_condition.success_stdout = Some(item.as_str().unwrap().to_owned());
        }

        let cur_key = "success_stderr";
        if let Some(item) = cfgmap.get(&cur_key) {
            if !item.is_str() {
                return _invalid_cfg(
                    &cur_key,
                    STR_UNKNOWN_VALUE,
                    ERR_INVALID_PARAMETER);
            }
            new_condition.success_stderr = Some(item.as_str().unwrap().to_owned());
        }

        let cur_key = "success_status";
        if let Some(item) = cfgmap.get(&cur_key) {
            if !item.is_int() {
                return _invalid_cfg(
                    &cur_key,
                    STR_UNKNOWN_VALUE,
                    ERR_INVALID_PARAMETER);
            }
            let i = *item.as_int().unwrap();
            if i < 0 || i as u64 > std::u32::MAX.into() {
                return _invalid_cfg(
                    &cur_key,
                    &item.as_str().unwrap(),
                    ERR_INVALID_PARAMETER);
            }
            new_condition.success_status = Some(i as u32);
        }

        let cur_key = "failure_stdout";
        if let Some(item) = cfgmap.get(&cur_key) {
            if !item.is_str() {
                return _invalid_cfg(
                    &cur_key,
                    STR_UNKNOWN_VALUE,
                    ERR_INVALID_PARAMETER);
            }
            new_condition.failure_stdout = Some(item.as_str().unwrap().to_owned());
        }

        let cur_key = "failure_stderr";
        if let Some(item) = cfgmap.get(&cur_key) {
            if !item.is_str() {
                return _invalid_cfg(
                    &cur_key,
                    STR_UNKNOWN_VALUE,
                    ERR_INVALID_PARAMETER);
            }
            new_condition.failure_stderr = Some(item.as_str().unwrap().to_owned());
        }

        let cur_key = "failure_status";
        if let Some(item) = cfgmap.get(&cur_key) {
            if !item.is_int() {
                return _invalid_cfg(
                    &cur_key,
                    STR_UNKNOWN_VALUE,
                    ERR_INVALID_PARAMETER);
            }
            let i = *item.as_int().unwrap();
            if i < 0 || i as u64 > std::u32::MAX.into() {
                return _invalid_cfg(
                    &cur_key,
                    &item.as_str().unwrap(),
                    ERR_INVALID_PARAMETER);
            }
            new_condition.failure_status = Some(i as u32);
        }

        // start the condition if the configuration did not suspend it
        if !new_condition.suspended {
            new_condition.start()?;
        }

        Ok(new_condition)
    }

}


impl Condition for CommandCondition {

    fn set_id(&mut self, id: i64) { self.cond_id = id; }
    fn get_name(&self) -> String { self.cond_name.clone() }
    fn get_id(&self) -> i64 { self.cond_id }
    fn get_type(&self) -> &str { "command" }


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
        self.suspended = true;
        Ok(true)
    }

    fn resume(&mut self) -> Result<bool, std::io::Error> {
        self.suspended = false;
        Ok(true)
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
    ///           the command based `CommandTask` task structure.
    fn _check_condition(&mut self) -> Result<Option<bool>, std::io::Error> {
        self.log(
            LogType::Debug,
            &format!("[START/MSG] checking command based condition")
        );
        // if the minimum interval between checks has been set, obey it
        // last_tested has already been set by trait to Instant::now()
        let t = self.last_tested.unwrap();
        if let Some(e) = self.check_after {
            if e > t - self.check_last {
                self.log(
                    LogType::Debug,
                    &format!("[START/MSG] check explicitly delayed by configuration")
                );
                return Ok(Some(false));
            }
        }

        // build the environment: least priority settings come first; it is
        // created as a hashmap in order to avoid duplicates, but it is
        // converted to Vec<&OsString, &OsString> in order to be passed to
        // subprocess::Popen::create in a subprocess::Popenconfig struct
        let mut temp_env: HashMap<String, String> = HashMap::new();

        // first inherit environment variables from underlying OS
        if self.include_env {
            for (var, value) in env::vars_os() {
                temp_env.insert(
                    String::from(var.to_str().unwrap()),
                    String::from(value.to_str().unwrap()),
                );
            }
        }

        // secondly add the custom task name and condition name variables
        if self.set_envvars {
            temp_env.insert(ENVVAR_NAME_COND.to_string(), String::from(&self.cond_name));
        }

        // at last insert user supplied variables
        for (var, value) in self.environment_vars.clone().into_iter() {
            temp_env.insert(var.clone(), value.clone());
        }

        // now convert everything to the required object (to be wrapped in Some())
        let mut shell_env: Vec<(OsString, OsString)> = Vec::new();
        for (var, value) in temp_env.into_iter() {
            shell_env.push((OsString::from(&var), OsString::from(&value)));
        }

        // build the subprocess configured environment
        let process_config = PopenConfig {
            //stdin: Redirection::Pipe,
            stdout: Redirection::Pipe,
            stderr: Redirection::Pipe,
            cwd: Some(OsString::from(self.startup_dir.as_os_str())),
            env: Some(shell_env),
            detached: false,
            ..Default::default()
        };

        // build the argv slice for Popen::create
        let mut process_argv: Vec<OsString> = Vec::new();
        process_argv.push(OsString::from(self.command.as_os_str()));
        for item in &self.args {
            process_argv.push(OsString::from(item));
        }

        self.log(LogType::Debug, &format!(
            "[START/MSG] running command: `{}`", self.command_line()));

        // run the process and capture possible errors
        let mut failure_reason: FailureReason = FailureReason::NoFailure;
        self._process_failed = false;
        self._process_status = 0;
        self._process_stderr = String::new();
        self._process_stdout = String::new();
        let startup_time = SystemTime::now();
        let open_process = Popen::create(&process_argv, process_config);
        if let Ok(mut process) = open_process {
            let proc_exit = process.wait();
            self._process_duration = SystemTime::now().duration_since(startup_time).unwrap();
            match proc_exit {
                Ok(exit_status) => {
                    let statusmsg: String;
                    if exit_status.success() {
                        // exit code is 0, and this usually indicates success
                        // however if it was not the expected exit code the
                        // failure reason has to be set to Status (for now);
                        // note thet also the case of exit code 0 considered
                        // as a failure status is taken into account here
                        statusmsg = String::from("OK/0");
                        self.log(LogType::Debug, &format!(
                            "[PROC/OK] command: `{}` exited with SUCCESS status {statusmsg}",
                            self.command_line()));
                        self._process_status = 0;
                        if let Some(expected) = self.success_status {
                            if expected != 0 {
                                self.log(LogType::Debug, &format!(
                                    "[PROC/OK] condition expected success exit code NOT matched: {expected}"));
                                failure_reason = FailureReason::Status;
                            }
                        } else if let Some(expectedf) = self.failure_status {
                            if expectedf == 0 {
                                self.log(LogType::Debug, &format!(
                                    "[PROC/OK] condition expected failure exit code matched: {expectedf}"));
                                failure_reason = FailureReason::Status;
                            }
                        }
                    } else {
                        match exit_status {
                            // exit code is nonzero, however this might be the
                            // expected behaviour of the executed command: if
                            // the exit code had to be checked then the check
                            // is performed with the following priority rule:
                            // 1. match resulting status for expected failure
                            // 2. match resulting status for unsuccessfulness
                            ExitStatus::Exited(v) => {
                                statusmsg = String::from(format!("ERROR/{v}"));
                                self.log(LogType::Debug, &format!(
                                    "[PROC/OK] command: `{}` exited with FAILURE status {statusmsg}",
                                    self.command_line()));
                                if let Some(expectedf) = self.failure_status {
                                    if v == expectedf {
                                        self.log(LogType::Debug, &format!(
                                            "[PROC/OK] condition expected failure exit code {expectedf} matched"));
                                        failure_reason = FailureReason::Status;
                                    } else if let Some(expected) = self.success_status {
                                        if v == expected {
                                            self.log(LogType::Debug, &format!(
                                                "[PROC/OK] condition expected success exit code {expected} matched"));
                                        } else {
                                            self.log(LogType::Debug, &format!(
                                                "[PROC/OK] condition expected success exit code {expected} NOT matched: {v}"));
                                            failure_reason = FailureReason::Status;
                                        }
                                    } else {
                                        self.log(LogType::Debug, &format!(
                                            "[PROC/OK] condition expected failure exit code {expectedf} matched"));
                                        failure_reason = FailureReason::Status;
                                    }
                                } else {
                                    if let Some(expected) = self.success_status {
                                        if v == expected {
                                            self.log(LogType::Debug, &format!(
                                                "[PROC/OK] condition expected success exit code {expected} matched"));
                                        } else {
                                            self.log(LogType::Debug, &format!(
                                                "[PROC/OK] condition expected success exit code {expected} NOT matched: {v}"));
                                            failure_reason = FailureReason::Status;
                                        }
                                    }
                                }
                                // if we are here, neither the success exit
                                // code nor the failure exit code were set by
                                // configuration, thus the status is still
                                // set to NoFailure
                            }
                            // if the subprocess did not exit properly it has
                            // to be considered as unsuccessful anyway: set the
                            // failure reason appropriately
                            ExitStatus::Signaled(v) => {
                                statusmsg = String::from(format!("SIGNAL/{v}"));
                                self.log(LogType::Warn, &format!(
                                    "[PROC/FAIL] command: `{}` ended for reason {statusmsg}",
                                    self.command_line()));
                                failure_reason = FailureReason::Other;
                            }
                            ExitStatus::Other(v) => {
                                statusmsg = String::from(format!("UNKNOWN/{v}"));
                                self.log(LogType::Warn, &format!(
                                    "[PROC/FAIL] command: `{}` ended for reason {statusmsg}",
                                    self.command_line()));
                                failure_reason = FailureReason::Other;
                            }
                            ExitStatus::Undetermined => {
                                statusmsg = String::from(format!("UNDETERMINED"));
                                self.log(LogType::Warn, &format!(
                                    "[PROC/FAIL] command: `{}` ended for reason {statusmsg}",
                                    self.command_line()));
                                failure_reason = FailureReason::Other;
                            }
                        }
                    }

                    // temporarily use the failure reason to determine whether
                    // or not to check for task success in the command output
                    match failure_reason {
                        // only when no other failure has occurred we harvest
                        // process IO and perform stdout/stderr text analysis
                        FailureReason::NoFailure => {
                            // command output based task result determination:
                            // both in regex matching and in direct text
                            // comparison the tests are performed in this
                            // order:
                            //   1. against expected success in stdout
                            //   2. against expected success in stderr
                            //   3. against expected failure in stdout
                            //   3. against expected failure in stderr
                            // if any of the tests does not fail, then the
                            // further test is performed; on the other side,
                            // failure in any of the tests causes skipping
                            // of all the following ones

                            let (out, err) = process.communicate(None)?;
                            if let Some(o) = out { self._process_stdout = o; }
                            if let Some(e) = err { self._process_stderr = e; }

                            // NOTE: in the following blocks, all the checks
                            // for failure_reason not to be NoFailure are
                            // needed to bail out if a failure condition has
                            // been already determined: this also enforces a
                            // check priority (as described above); the first
                            // of these checks is pleonastic because NoFailure
                            // has been just matched, however it improves code
                            // modularity and readability, and possibility to
                            // change priority by just moving code: it has
                            // little cost compared to this so we keep it

                            // A. regular expresion checks: case sensitiveness
                            //    is directly handled by the Regex engine
                            if self.match_regexp {
                                // A.1 regex success stdout check
                                if failure_reason == FailureReason::NoFailure {
                                    if let Some(p) = &self.success_stdout { if !p.is_empty() {
                                        if let Ok(re) = regex::RegexBuilder::new(p)
                                            .case_insensitive(!self.case_sensitive).build() {
                                            if self.match_exact {
                                                if re.is_match(&self._process_stdout) {
                                                    self.log(LogType::Debug, &format!(
                                                        "[PROC/OK] condition success stdout (regex) {:?} matched", p));
                                                } else {
                                                    self.log(LogType::Debug, &format!(
                                                        "[PROC/OK] condition success stdout (regex) {:?} NOT matched", p));
                                                    failure_reason = FailureReason::StdOut;
                                                }
                                            } else {
                                                if let Some(_) = re.find(&self._process_stdout) {
                                                    self.log(LogType::Debug, &format!(
                                                        "[PROC/OK] condition success stdout (regex) {:?} found", p));
                                                } else {
                                                    self.log(LogType::Debug, &format!(
                                                        "[PROC/OK] condition success stdout (regex) {:?} NOT found", p));
                                                    failure_reason = FailureReason::StdOut;
                                                }
                                            }
                                        } else {
                                            self.log(LogType::Error, &format!(
                                                "[PROC/FAIL] provided INVALID stdout regex {:?} NOT found/matched", p));
                                            failure_reason = FailureReason::StdOut;
                                        }}
                                    }
                                }
                                // A.2 regex success stderr check
                                if failure_reason == FailureReason::NoFailure {
                                    if let Some(p) = &self.success_stderr { if !p.is_empty() {
                                        if let Ok(re) = regex::RegexBuilder::new(p)
                                            .case_insensitive(!self.case_sensitive).build() {
                                            if self.match_exact {
                                                if re.is_match(&self._process_stderr) {
                                                    self.log(LogType::Debug, &format!(
                                                        "[PROC/OK] condition success stderr (regex) {:?} matched", p));
                                                } else {
                                                    self.log(LogType::Debug, &format!(
                                                        "[PROC/OK] condition success stderr (regex) {:?} NOT matched", p));
                                                    failure_reason = FailureReason::StdErr;
                                                }
                                            } else {
                                                if let Some(_) = re.find(&self._process_stderr) {
                                                    self.log(LogType::Debug, &format!(
                                                        "[PROC/OK] condition success stderr (regex) {:?} found", p));
                                                } else {
                                                    self.log(LogType::Debug, &format!(
                                                        "[PROC/OK] condition success stderr (regex) {:?} NOT found", p));
                                                    failure_reason = FailureReason::StdErr;
                                                }
                                            }
                                        } else {
                                            self.log(LogType::Error, &format!(
                                                "[PROC/FAIL] provided INVALID stderr regex {:?} NOT found/matched", p));
                                            failure_reason = FailureReason::StdErr;
                                        }}
                                    }
                                }
                                // A.3 regex failure stdout check
                                if failure_reason == FailureReason::NoFailure {
                                    if let Some(p) = &self.failure_stdout { if !p.is_empty() {
                                        if let Ok(re) = regex::RegexBuilder::new(p)
                                            .case_insensitive(!self.case_sensitive).build() {
                                            if self.match_exact {
                                                if re.is_match(&self._process_stdout) {
                                                    self.log(LogType::Debug, &format!(
                                                        "[PROC/OK] condition failure stdout (regex) {:?} matched", p));
                                                    failure_reason = FailureReason::StdOut;
                                                } else {
                                                    self.log(LogType::Debug, &format!(
                                                        "[PROC/OK] condition failure stdout (regex) {:?} NOT matched", p));
                                                }
                                            } else {
                                                if let Some(_) = re.find(&self._process_stdout) {
                                                    self.log(LogType::Debug, &format!(
                                                        "[PROC/OK] condition failure stdout (regex) {:?} found", p));
                                                    failure_reason = FailureReason::StdOut;
                                                } else {
                                                    self.log(LogType::Debug, &format!(
                                                        "[PROC/OK] condition failure stdout (regex) {:?} NOT found", p));
                                                }
                                            }
                                        } else {
                                            self.log(LogType::Error, &format!(
                                                "[PROC/FAIL] provided INVALID failure stdout regex {:?} NOT found/matched", p));
                                        }}
                                    }
                                }
                                // A.4 regex failure stderr check
                                if failure_reason == FailureReason::NoFailure {
                                    if let Some(p) = &self.failure_stderr { if !p.is_empty() {
                                        if let Ok(re) = regex::RegexBuilder::new(p)
                                            .case_insensitive(!self.case_sensitive).build() {
                                            if self.match_exact {
                                                if re.is_match(&self._process_stderr) {
                                                    self.log(LogType::Debug, &format!(
                                                        "[PROC/OK] condition success stderr (regex) {:?} matched", p));
                                                    failure_reason = FailureReason::StdErr;
                                                } else {
                                                    self.log(LogType::Debug, &format!(
                                                        "[PROC/OK] condition success stderr (regex) {:?} NOT matched", p));
                                                }
                                            } else {
                                                if let Some(_) = re.find(&self._process_stderr) {
                                                    self.log(LogType::Debug, &format!(
                                                        "[PROC/OK] condition success stderr (regex) {:?} found", p));
                                                    failure_reason = FailureReason::StdErr;
                                                } else {
                                                    self.log(LogType::Debug, &format!(
                                                        "[PROC/OK] condition success stderr (regex) {:?} NOT found", p));
                                                }
                                            }
                                        } else {
                                            self.log(LogType::Error, &format!(
                                                "[PROC/FAIL] provided INVALID stderr regex {:?} NOT found/matched", p));
                                        }}
                                    }
                                }
                            } else {
                                // B. text checks: the case sensitive and case
                                //    insensitive options are handled separately
                                //    because they require different comparisons
                                if self.case_sensitive {
                                    // B.1a CS text success stdout check
                                    if failure_reason == FailureReason::NoFailure {
                                        if let Some(p) = &self.success_stdout { if !p.is_empty() {
                                            if self.match_exact {
                                                if self._process_stdout == *p {
                                                    self.log(LogType::Debug, &format!(
                                                        "[PROC/OK] condition success stdout {:?} matched", p));
                                                } else {
                                                    self.log(LogType::Debug, &format!(
                                                        "[PROC/OK] condition success stdout {:?} NOT matched", p));
                                                    failure_reason = FailureReason::StdOut;
                                                }
                                            } else {
                                                if self._process_stdout.contains(p) {
                                                    self.log(LogType::Debug, &format!(
                                                        "[PROC/OK] condition success stdout {:?} found", p));
                                                } else {
                                                    self.log(LogType::Debug, &format!(
                                                        "[PROC/OK] condition success stdout {:?} NOT found", p));
                                                    failure_reason = FailureReason::StdOut;
                                                }
                                            }
                                        }}
                                    }
                                    // B.2a CS text success stderr check
                                    if failure_reason == FailureReason::NoFailure {
                                        if let Some(p) = &self.success_stderr { if !p.is_empty() {
                                            if self.match_exact {
                                                if self._process_stderr == *p {
                                                    self.log(LogType::Debug, &format!(
                                                        "[PROC/OK] condition success stderr {:?} matched", p));
                                                } else {
                                                    self.log(LogType::Debug, &format!(
                                                        "[PROC/OK] condition success stderr {:?} NOT matched", p));
                                                    failure_reason = FailureReason::StdErr;
                                                }
                                            } else {
                                                if self._process_stderr.contains(p) {
                                                    self.log(LogType::Debug, &format!(
                                                        "[PROC/OK] condition success stderr {:?} found", p));
                                                } else {
                                                    self.log(LogType::Debug, &format!(
                                                        "[PROC/OK] condition success stderr {:?} NOT found", p));
                                                    failure_reason = FailureReason::StdErr;
                                                }
                                            }
                                        }}
                                    }
                                    // B.3a CS text failure stdout check
                                    if failure_reason == FailureReason::NoFailure {
                                        if let Some(p) = &self.failure_stdout { if !p.is_empty() {
                                            if self.match_exact {
                                                if self._process_stdout == *p {
                                                    self.log(LogType::Debug, &format!(
                                                        "[PROC/OK] condition failure stdout {:?} matched", p));
                                                    failure_reason = FailureReason::StdOut;
                                                } else {
                                                    self.log(LogType::Debug, &format!(
                                                        "[PROC/OK] condition failure stdout {:?} NOT matched", p));
                                                }
                                            } else {
                                                if self._process_stdout.contains(p) {
                                                    self.log(LogType::Debug, &format!(
                                                        "[PROC/OK] condition failure stdout {:?} found", p));
                                                    failure_reason = FailureReason::StdOut;
                                                } else {
                                                    self.log(LogType::Debug, &format!(
                                                        "[PROC/OK] condition failure stdout {:?} NOT found", p));
                                                }
                                            }
                                        }}
                                    }
                                    // B.4a CS text failure stderr check
                                    if failure_reason == FailureReason::NoFailure {
                                        if let Some(p) = &self.failure_stderr { if !p.is_empty() {
                                            if self.match_exact {
                                                if self._process_stderr == *p {
                                                    self.log(LogType::Debug, &format!(
                                                        "[PROC/OK] condition failure stderr {:?} matched", p));
                                                    failure_reason = FailureReason::StdErr;
                                                } else {
                                                    self.log(LogType::Debug, &format!(
                                                        "[PROC/OK] condition failure stderr {:?} NOT matched", p));
                                                }
                                            } else {
                                                if self._process_stderr.contains(p) {
                                                    self.log(LogType::Debug, &format!(
                                                        "[PROC/OK] condition failure stderr {:?} found", p));
                                                    failure_reason = FailureReason::StdErr;
                                                } else {
                                                    self.log(LogType::Debug, &format!(
                                                        "[PROC/OK] condition failure stderr {:?} NOT found", p));
                                                }
                                            }
                                        }}
                                    }
                                } else {
                                    // B.1b CI text success stdout check
                                    if failure_reason == FailureReason::NoFailure {
                                        if let Some(p) = &self.success_stdout { if !p.is_empty() {
                                            if self.match_exact {
                                                if self._process_stdout.to_uppercase() == p.to_uppercase() {
                                                    self.log(LogType::Debug, &format!(
                                                        "[PROC/OK] condition success stdout {:?} matched", p));
                                                } else {
                                                    self.log(LogType::Debug, &format!(
                                                        "[PROC/OK] condition success stdout {:?} NOT matched", p));
                                                    failure_reason = FailureReason::StdOut;
                                                }
                                            } else {
                                                if self._process_stdout.to_uppercase().contains(&p.to_uppercase()) {
                                                    self.log(LogType::Debug, &format!(
                                                        "[PROC/OK] condition success stdout {:?} found", p));
                                                } else {
                                                    self.log(LogType::Debug, &format!(
                                                        "[PROC/OK] condition success stdout {:?} NOT found", p));
                                                    failure_reason = FailureReason::StdOut;
                                                }
                                            }
                                        }}
                                    }
                                    // B.2b CI text success stderr check
                                    if failure_reason == FailureReason::NoFailure {
                                        if let Some(p) = &self.success_stderr { if !p.is_empty() {
                                            if self.match_exact {
                                                if self._process_stderr.to_uppercase() == p.to_uppercase() {
                                                    self.log(LogType::Debug, &format!(
                                                        "[PROC/OK] condition success stderr {:?} matched", p));
                                                } else {
                                                    self.log(LogType::Debug, &format!(
                                                        "[PROC/OK] condition success stderr {:?} NOT matched", p));
                                                    failure_reason = FailureReason::StdErr;
                                                }
                                            } else {
                                                if self._process_stderr.to_uppercase().contains(&p.to_uppercase()) {
                                                    self.log(LogType::Debug, &format!(
                                                        "[PROC/OK] condition success stderr {:?} found", p));
                                                } else {
                                                    self.log(LogType::Debug, &format!(
                                                        "[PROC/OK] condition success stderr {:?} NOT found", p));
                                                    failure_reason = FailureReason::StdErr;
                                                }
                                            }
                                        }}
                                    }
                                    // B.3b CI text failure stdout check
                                    if failure_reason == FailureReason::NoFailure {
                                        if let Some(p) = &self.failure_stdout { if !p.is_empty() {
                                            if self.match_exact {
                                                if self._process_stdout.to_uppercase() == p.to_uppercase() {
                                                    self.log(LogType::Debug, &format!(
                                                        "[PROC/OK] condition failure stdout {:?} matched", p));
                                                    failure_reason = FailureReason::StdOut;
                                                } else {
                                                    self.log(LogType::Debug, &format!(
                                                        "[PROC/OK] condition failure stdout {:?} NOT matched", p));
                                                }
                                            } else {
                                                if self._process_stdout.to_uppercase().contains(&p.to_uppercase()) {
                                                    self.log(LogType::Debug, &format!(
                                                        "[PROC/OK] condition failure stdout {:?} found", p));
                                                    failure_reason = FailureReason::StdOut;
                                                } else {
                                                    self.log(LogType::Debug, &format!(
                                                        "[PROC/OK] condition failure stdout {:?} NOT found", p));
                                                }
                                            }
                                        }}
                                    }
                                    // B.4b CI text failure stderr check
                                    if failure_reason == FailureReason::NoFailure {
                                        if let Some(p) = &self.failure_stderr { if !p.is_empty() {
                                            if self.match_exact {
                                                if self._process_stderr.to_uppercase() == p.to_uppercase() {
                                                    self.log(LogType::Debug, &format!(
                                                        "[PROC/OK] condition failure stderr {:?} matched", p));
                                                    failure_reason = FailureReason::StdErr;
                                                } else {
                                                    self.log(LogType::Debug, &format!(
                                                        "[PROC/OK] condition failure stderr {:?} NOT matched", p));
                                                }
                                            } else {
                                                if self._process_stderr.to_uppercase().contains(&p.to_uppercase()) {
                                                    self.log(LogType::Debug, &format!(
                                                        "[PROC/OK] condition failure stderr {:?} found", p));
                                                    failure_reason = FailureReason::StdErr;
                                                } else {
                                                    self.log(LogType::Debug, &format!(
                                                        "[PROC/OK] condition failure stderr {:?} NOT found", p));
                                                }
                                            }
                                        }}
                                    }
                                }
                            }
                        }
                        _ => { /* no need to test for other failures */ }
                    }
                }

                // the command could not be executed thus an error is reported
                Err(e) => {
                    self._process_failed = true;
                    self.log(LogType::Error, &format!(
                        "[END/FAIL] could not execute command: `{}` (reason: {e})",
                        self.command_line()));
                    return Err(Error::new(
                        ErrorKind::Unsupported,
                        format!(
                        "condition {}/[{}] could not execute command",
                        self.cond_name,
                        self.cond_id,
                    )));
                }
            }
        } else {
            // something happened before the command could be run
            if let Err(e) = open_process {
                self._process_failed = true;
                self.log(LogType::Error, &format!(
                    "[END/FAIL] could not execute command: `{}` (reason: {e})",
                    self.command_line()));
            } else {
                self._process_failed = true;
                self.log(LogType::Error, &format!(
                    "[END/FAIL] could not execute command: `{}` (reason: unknown)",
                    self.command_line()));
            }
            return Err(Error::new(
                ErrorKind::Unsupported,
                format!(
                "condition {}/[{}] could not execute command",
                self.cond_name,
                self.cond_id,
            )));
        }

        // now the time of the last check can be set to the actual time in
        // order to allow further checks to comply with the request to be
        // only run at certain intervals
        self.check_last = t;

        // return true on success and false otherwise
        match failure_reason {
            FailureReason::NoFailure => {
                self.log(LogType::Debug, &String::from(
                    format!("[END/OK] condition checked successfully in {:.2}s",
                    self._process_duration.as_secs_f64())));
                Ok(Some(true))
            }
            FailureReason::StdOut => {
                self._process_failed = true;
                self.log(LogType::Info, &String::from(
                    format!("[END/FAIL] condition checked unsuccessfully (stdout check) in {:.2}s",
                    self._process_duration.as_secs_f64())));
                Ok(Some(false))
            }
            FailureReason::StdErr => {
                self._process_failed = true;
                self.log(LogType::Info, &String::from(
                    format!("[END/FAIL] condition checked unsuccessfully (stderr check) in {:.2}s",
                    self._process_duration.as_secs_f64())));
                Ok(Some(false))
            }
            FailureReason::Status => {
                self._process_failed = true;
                self.log(LogType::Info, &String::from(
                    format!("[END/FAIL] condition checked unsuccessfully (status check) in {:.2}s",
                    self._process_duration.as_secs_f64())));
                Ok(Some(false))
            }
            FailureReason::Other => {
                self._process_failed = true;
                self.log(LogType::Info, &String::from(
                    format!("[END/FAIL] task ended unexpectedly in {:.2}s",
                    self._process_duration.as_secs_f64())));
                Ok(Some(false))
            }
        }
    }

}




// end.
