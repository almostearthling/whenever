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
//! variables. If requested, the condition can set an extra variable set to
//! its name, so that commands or scripts that are aware of being invoked by
//! the application can have further info about the context.


use std::collections::HashMap;
use std::env;
use std::io::ErrorKind;
use std::time::{Instant, SystemTime, Duration};
use std::path::PathBuf;
use std::ffi::OsString;
use std::hash::{DefaultHasher, Hash, Hasher};

use itertools::Itertools;

use subprocess::{Popen, PopenConfig, Redirection, ExitStatus, PopenError};

use cfgmap::CfgMap;

use super::base::Condition;
use crate::task::registry::TaskRegistry;
use crate::common::logging::{log, LogType};
use crate::{cfg_mandatory, constants::*};

use crate::cfghelp::*;



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
    timeout: Option<Duration>,

    // internal values
    check_last: Instant,
    _process_stdout: String,
    _process_stderr: String,
    _process_status: u32,
    _process_failed: bool,
    _process_duration: Duration,
}


// implement the hash protocol
impl Hash for CommandCondition {
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
        self.command.hash(state);
        self.args.hash(state);
        self.startup_dir.hash(state);
        self.match_exact.hash(state);
        self.match_regexp.hash(state);
        self.case_sensitive.hash(state);
        self.include_env.hash(state);
        self.set_envvars.hash(state);
        self.timeout.hash(state);

        // 0 is hashed on the else branch because if we get two items, for
        // instance, one of which has only success_stdout defined as a string
        // and the other which has only success_stderr defined as the very
        // same string, they might have the same hash being in fact different
        if let Some(x) = self.success_status {
            x.hash(state);
        } else {
            0.hash(state);
        }
        if let Some(x) = &self.success_stdout {
            x.hash(state);
        } else {
            0.hash(state);
        }
        if let Some(x) = &self.success_stderr {
            x.hash(state);
        } else {
            0.hash(state);
        }

        if let Some(x) = self.failure_status {
            x.hash(state);
        } else {
            0.hash(state);
        }
        if let Some(x) = &self.failure_stdout {
            x.hash(state);
        } else {
            0.hash(state);
        }
        if let Some(x) = &self.failure_stderr {
            x.hash(state);
        } else {
            0.hash(state);
        }

        // the keys are sorted because the order in which environment vars
        // are defined is actually not significant
        for key in self.environment_vars.keys().sorted() {
            key.hash(state);
            self.environment_vars[key].hash(state);
        }
    }
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
        log(
            LogType::Debug,
            LOG_EMITTER_CONDITION_COMMAND,
            LOG_ACTION_NEW,
            Some((name, 0)),
            LOG_WHEN_INIT,
            LOG_STATUS_MSG,
            &format!("CONDITION {name}: creating a new command based condition"),
        );
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
            timeout: None,

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
            if v.contains(' ') || v.contains('\t') || v.contains('\n') || v.contains('\r') {
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

    /// State that the first check and possible following tests are to be
    /// performed after a certain amount of time. This option is present in
    /// this type of condition because the test itself can be both time and
    /// resource consuming, and an user may choose to avoid to perform it
    /// at every tick.
    pub fn checks_after(mut self, delta: Duration) -> Self {
        self.check_after = Some(delta);
        self
    }

    /// Set a variable in the custom environment that the command base task
    /// provides to the spawned process.
    ///
    /// # Arguments
    ///
    /// * `var` - the variable name
    /// * `value` - the value assigned to the named variable
    pub fn set_variable(&mut self, var: &str, value: &str) -> Option<String> {
        self.environment_vars.insert(String::from(var), String::from(value))
    }

    /// Unset a variable in the custom environment that the command base task
    /// provides to the spawned process.
    ///
    /// # Arguments
    ///
    /// * `var` - the name of the variable to be unset
    pub fn unset_variable(&mut self, var: &str) -> Option<String> {
        self.environment_vars.remove(var)
    }


    /// Constructor modifier to include or exclude existing environment: if
    /// the parameter is set to `false`, the original environment is not passed
    /// to the spawned process. Default behaviour is to pass the environment.
    pub fn includes_env(mut self, yes: bool) -> Self {
        self.include_env = yes;
        self
    }

    /// Constructor modifier to specify that the values to match against the
    /// output of the spawned process have to be considered as regular
    /// expressions when the argument is set to `true`. The default behaviour
    /// is to consider them as simple strings.
    pub fn matches_regexp(mut self, yes: bool) -> Self {
        self.match_regexp = yes;
        self
    }

    /// Constructor modifier to specify that the entire output of the spawned
    /// process must match against the provided value, when the argument is set
    /// to `true`. The default behaviour is to _partially_ match the output.
    pub fn matches_exact(mut self, yes: bool) -> Self {
        self.match_exact = yes;
        self
    }

    /// Constructor modifier to specify that the matching against the output of
    /// the spawned command is to be performed case-sensitively when set to
    /// `true`. The default behaviour is to match ignoring case.
    pub fn matches_case(mut self, yes: bool) -> Self {
        self.case_sensitive = yes;
        self
    }

    /// Constructor modifier to specify that the task should not set the
    /// environment variables that specify the task name and the condition that
    /// triggered the task, when set to `false`. The default behaviour is to
    /// export those variables.
    pub fn sets_envvars(mut self, yes: bool) -> Self {
        self.set_envvars = yes;
        self
    }

    /// Constructor modifier to possibly set the values that have to match
    /// the spawned process' standard output in order for the execution to be
    /// considered successful: this can be a simple string or a regular
    /// expression pattern, according to the value provided with the
    /// `matches_regexp` constructor modifier.
    pub fn expects_stdout(mut self, s: &str) -> Self {
        self.success_stdout = Some(s.to_string());
        self
    }

    /// Constructor modifier to possibly set the values that have to match
    /// the spawned process' standard error in order for the execution to be
    /// considered successful: this can be a simple string or a regular
    /// expression pattern, according to the value provided with the
    /// `matches_regexp` constructor modifier.
    pub fn expects_stderr(mut self, s: &str) -> Self {
        self.success_stderr = Some(s.to_string());
        self
    }

    /// Constructor modifier to possibly set the values that have to match
    /// the spawned process' standard output in order for the execution to be
    /// considered failed: this can be a simple string or a regular expression
    /// pattern, according to the value provided with the `matches_regexp`
    /// constructor modifier.
    pub fn rejects_stdout(mut self, s: &str) -> Self {
        self.failure_stdout = Some(s.to_string());
        self
    }

    /// Constructor modifier to possibly set the values that have to match
    /// the spawned process' standard error in order for the execution to be
    /// considered failed: this can be a simple string or a regular expression
    /// pattern, according to the value provided with the `matches_regexp`
    /// constructor modifier.
    pub fn rejects_stderr(mut self, s: &str) -> Self {
        self.failure_stderr = Some(s.to_string());
        self
    }

    /// Constructor modifier to possibly set the value that have to match
    /// the spawned process' exit code in order for the execution to be
    /// considered successful.
    pub fn expects_exitcode(mut self, c: u32) -> Self {
        self.success_status = Some(c);
        self
    }

    /// Constructor modifier to possibly set the value that have to match
    /// the spawned process' exit code in order for the execution to be
    /// considered failed.
    pub fn rejects_exitcode(mut self, c: u32) -> Self {
        self.failure_status = Some(c);
        self
    }

    /// If set, command execution times out after specified duration
    pub fn times_out_after(mut self, delta: Duration) -> Self {
        self.timeout = Some(delta);
        self
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
    /// timeout_seconds = 30
    ///
    /// case_sensitive = false
    /// include_environment = true
    /// set_environment_variables = true
    /// environment_variables = { VARNAME1 = "value1", VARNAME2 = "value2", ... }
    /// ```
    ///
    /// Any incorrect value will cause an error. The value of the `type` entry
    /// *must* be set to `"command"` mandatorily for this type of `Condition`.
    pub fn load_cfgmap(cfgmap: &CfgMap, task_registry: &'static TaskRegistry) -> std::io::Result<CommandCondition> {

        let check = vec![
            "type",
            "name",
            "tags",
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
            "set_environment_variables",
            "environment_variables",
            "check_after",
            "success_stdout",
            "success_stderr",
            "success_status",
            "failure_stdout",
            "failure_stderr",
            "failure_status",
            "timeout_seconds",
        ];
        cfg_check_keys(cfgmap, &check)?;

        // type and name are both mandatory but type is only checked
        cfg_mandatory!(cfg_string_check_exact(cfgmap, "type", "command"))?;
        let name = cfg_mandatory!(cfg_string_check_regex(cfgmap, "name", &RE_COND_NAME))?.unwrap();

        // specific mandatory parameter retrieval
        let command = PathBuf::from(cfg_mandatory!(cfg_string(cfgmap, "command"))?.unwrap());
        let args = cfg_mandatory!(cfg_vec_string(cfgmap, "command_arguments"))?.unwrap();
        let startup_path = PathBuf::from(cfg_mandatory!(cfg_string(cfgmap, "startup_path"))?.unwrap());
        if !startup_path.is_dir() {
            return Err(cfg_err_invalid_config(
                "startup_path",
                &startup_path.to_string_lossy(),
                ERR_INVALID_STARTUP_PATH,
            ));
        };

        // initialize the structure
        let mut new_condition = CommandCondition::new(
            &name,
            &command,
            &args,
            &startup_path,
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
        if let Some(v) = cfg_bool(cfgmap, "match_exact")? {
            new_condition.match_exact = v;
        }
        if let Some(v) = cfg_bool(cfgmap, "match_regular_expression")? {
            new_condition.match_regexp = v;
        }
        if let Some(v) = cfg_bool(cfgmap, "case_sensitive")? {
            new_condition.case_sensitive = v;
        }
        if let Some(v) = cfg_bool(cfgmap, "include_environment")? {
            new_condition.include_env = v;
        }
        if let Some(v) = cfg_bool(cfgmap, "set_environment_variables")? {
            new_condition.set_envvars = v;
        }

        // the environment variable case is peculiar and has no shortcut
        let cur_key = "environment_variables";
        if let Some(item) = cfgmap.get(cur_key) {
            if !item.is_map() {
                return Err(cfg_err_invalid_config(
                    cur_key,
                    STR_UNKNOWN_VALUE,
                    ERR_INVALID_PARAMETER,
                ));
            } else {
                let map = item.as_map().unwrap();
                let mut vars: HashMap<String, String> = HashMap::new();
                for name in map.keys() {
                    if !RE_ENVVAR_NAME.is_match(name) {
                        return Err(cfg_err_invalid_config(
                            cur_key,
                            &name,
                            ERR_INVALID_ENVVAR_NAME,
                        ));
                    } else if let Some(value) = map.get(name) {
                        if value.is_str() || value.is_int() || value.is_float() || value.is_datetime() {
                            vars.insert(name.to_string(), value.as_str().unwrap().to_string());
                        } else {
                            return Err(cfg_err_invalid_config(
                                cur_key,
                                STR_UNKNOWN_VALUE,
                                ERR_INVALID_ENVVAR_VALUE,
                            ));
                        }
                    } else {
                        return Err(cfg_err_invalid_config(
                            cur_key,
                            STR_UNKNOWN_VALUE,
                            ERR_INVALID_ENVVAR_NAME,
                        ));
                    }
                }
                new_condition.environment_vars = vars;
            }
        }

        if let Some(v) = cfg_int_check_above_eq(cfgmap, "check_after", 1)? {
            new_condition.check_after = Some(Duration::from_secs(v as u64));
        }

        new_condition.success_stdout = cfg_string(cfgmap, "success_stdout")?;
        new_condition.success_stderr = cfg_string(cfgmap, "success_stderr")?;
        if let Some(v) = cfg_int_check_interval(cfgmap, "success_status", 0, std::u32::MAX as i64)? {
            new_condition.success_status = Some(v as u32);
        }

        new_condition.failure_stdout = cfg_string(cfgmap, "failure_stdout")?;
        new_condition.failure_stderr = cfg_string(cfgmap, "failure_stderr")?;
        if let Some(v) = cfg_int_check_interval(cfgmap, "failure_status", 0, std::u32::MAX as i64)? {
            new_condition.failure_status = Some(v as u32);
        }

        if let Some(v) = cfg_int_check_above_eq(cfgmap, "timeout_seconds", 0)? {
            if v > 0 {
                new_condition.timeout = Some(Duration::from_secs(v as u64));
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
    pub fn check_cfgmap(cfgmap: &CfgMap, available_tasks: &Vec<&str>) -> std::io::Result<String> {

        let check = vec![
            "type",
            "name",
            "tags",
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
            "set_environment_variables",
            "environment_variables",
            "check_after",
            "success_stdout",
            "success_stderr",
            "success_status",
            "failure_stdout",
            "failure_stderr",
            "failure_status",
            "timeout_seconds",
        ];
        cfg_check_keys(cfgmap, &check)?;

        // common mandatory parameter check

        // type and name are both mandatory: type is checked and name is kept
        cfg_mandatory!(cfg_string_check_exact(cfgmap, "type", "command"))?;
        let name = cfg_mandatory!(cfg_string_check_regex(cfgmap, "name", &RE_EVENT_NAME))?.unwrap();

        // specific mandatory parameter check
        cfg_mandatory!(cfg_string(cfgmap, "command"))?;
        cfg_mandatory!(cfg_vec_string(cfgmap, "command_arguments"))?;
        let startup_path = PathBuf::from(cfg_mandatory!(cfg_string(cfgmap, "startup_path"))?.unwrap());
        if !startup_path.is_dir() {
            return Err(cfg_err_invalid_config(
                "startup_path",
                &startup_path.to_string_lossy(),
                ERR_INVALID_STARTUP_PATH,
            ));
        };


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
        cfg_bool(cfgmap, "execute_sequence")?;
        cfg_bool(cfgmap, "break_on_failure")?;
        cfg_bool(cfgmap, "break_on_success")?;
        cfg_bool(cfgmap, "suspended")?;

        cfg_int_check_above_eq(cfgmap, "check_after", 1)?;

        cfg_bool(cfgmap, "match_exact")?;
        cfg_bool(cfgmap, "match_regular_expression")?;
        cfg_bool(cfgmap, "case_sensitive")?;
        cfg_bool(cfgmap, "include_environment")?;
        cfg_bool(cfgmap, "set_environment_variables")?;

        // the environment variables case is peculiar and has no shortcut
        let cur_key = "environment_variables";
        if let Some(item) = cfgmap.get(cur_key) {
            if !item.is_map() {
                return Err(cfg_err_invalid_config(
                    cur_key,
                    STR_UNKNOWN_VALUE,
                    ERR_INVALID_PARAMETER,
                ));
            } else {
                let map = item.as_map().unwrap();
                let mut vars: HashMap<String, String> = HashMap::new();
                for name in map.keys() {
                    if !RE_VAR_NAME.is_match(name) {
                        return Err(cfg_err_invalid_config(
                            cur_key,
                            &name,
                            ERR_INVALID_ENVVAR_NAME,
                        ));
                    } else if let Some(value) = map.get(name) {
                        if value.is_str() || value.is_int() || value.is_float() || value.is_datetime() {
                            vars.insert(name.to_string(), value.as_str().unwrap().to_string());
                        } else {
                            return Err(cfg_err_invalid_config(
                                cur_key,
                                STR_UNKNOWN_VALUE,
                                ERR_INVALID_ENVVAR_VALUE,
                            ));
                        }
                    } else {
                        return Err(cfg_err_invalid_config(
                            cur_key,
                            STR_UNKNOWN_VALUE,
                            ERR_INVALID_ENVVAR_NAME,
                        ));
                    }
                }
            }
        }

        cfg_string(cfgmap, "success_stdout")?;
        cfg_string(cfgmap, "success_stderr")?;
        cfg_int_check_interval(cfgmap, "success_status", 0, std::u32::MAX as i64)?;

        cfg_string(cfgmap, "failure_stdout")?;
        cfg_string(cfgmap, "failure_stderr")?;
        cfg_int_check_interval(cfgmap, "failure_status", 0, std::u32::MAX as i64)?;

        cfg_int_check_above_eq(cfgmap, "timeout_seconds", 0)?;

        Ok(name)
    }

}


impl Condition for CommandCondition {

    fn set_id(&mut self, id: i64) { self.cond_id = id; }
    fn get_name(&self) -> String { self.cond_name.clone() }
    fn get_id(&self) -> i64 { self.cond_id }
    fn get_type(&self) -> &str { "command" }

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
    ///           the command based `CommandTask` task structure.
    fn _check_condition(&mut self) -> Result<Option<bool>, std::io::Error> {

        // helper to start a process (in the same thread), read stdout/stderr
        // continuously (thus freeing its buffers), optionally terminate it
        // after a certain timeout has been reached: it returns a tuple
        // consisting of the exit status and, optionally, strings containing
        // stdout and stderr contents; no way is given of providing input to
        // the subprocess
        fn spawn_process(
            mut proc: Popen,
            poll_interval: Duration,
            timeout: Option<Duration>,
        ) -> Result<(ExitStatus, Option<String>, Option<String>), std::io::Error> {
            let mut stdout = String::new();
            let mut stderr = String::new();
            let mut out;
            let mut err;
            let mut exit_status;
            let mut comm = proc.communicate_start(None).limit_time(poll_interval);
            let startup = SystemTime::now();

            loop {
                // we intercept timeout error here because we just could be
                // waiting for more output to be available; the timed_out
                // flag is used to avoid waiting extra time when reading
                // from stdout/stderr has already had a cost in this terms
                let mut timed_out = false;
                let cres = comm.read_string();
                if cres.is_err() {
                    if cres.as_ref().unwrap_err().kind() == std::io::ErrorKind::TimedOut {
                        let (co, ce) = cres.as_ref().unwrap_err().capture.clone();
                        timed_out = true;
                        if co.is_some() {
                            out = Some(String::from_utf8(co.unwrap()).unwrap_or_default());
                        } else {
                            out = None;
                        }
                        if ce.is_some() {
                            err = Some(String::from_utf8(ce.unwrap()).unwrap_or_default());
                        } else {
                            err = None;
                        }
                    } else {
                        return Err(std::io::Error::new(
                            cres.as_ref().unwrap_err().kind(),
                            cres.as_ref().unwrap_err().to_string(),
                        ));
                    }
                } else {
                    (out, err) = cres.unwrap();
                }

                if let Some(ref o) = out { stdout.push_str(o.as_str()); }
                if let Some(ref e) = err { stderr.push_str(e.as_str()); }
                exit_status = proc.poll();
                if exit_status.is_none() {
                    if let Some(t) = timeout {
                        if SystemTime::now() > startup + t {
                            let res = proc.terminate();
                            if res.is_err() {
                                let _ = proc.kill();
                            }
                            return Err(std::io::Error::new(
                                std::io::ErrorKind::TimedOut,
                                ERR_TIMEOUT_REACHED,
                            ));
                        }
                    }
                } else {
                    break;
                }
                if !timed_out {
                    std::thread::sleep(poll_interval);
                }
            }

            // same as above
            let cres = comm.read_string();
            if cres.is_err() {
                if cres.as_ref().unwrap_err().kind() == std::io::ErrorKind::TimedOut {
                    let (co, ce) = cres.as_ref().unwrap_err().capture.clone();
                    if co.is_some() {
                        out = Some(String::from_utf8(co.unwrap()).unwrap_or_default());
                    } else {
                        out = None;
                    }
                    if ce.is_some() {
                        err = Some(String::from_utf8(ce.unwrap()).unwrap_or_default());
                    } else {
                        err = None;
                    }
                } else {
                    return Err(std::io::Error::new(
                        cres.as_ref().unwrap_err().kind(),
                        cres.as_ref().unwrap_err().to_string(),
                    ));
                }
            } else {
                (out, err) = cres.unwrap();
            }
            if let Some(ref o) = out { stdout.push_str(o); }
            if let Some(ref e) = err { stderr.push_str(e); }
            if let Some(exit_status) = exit_status {
                Ok((
                    exit_status,
                    { if !stdout.is_empty() { Some(stdout) } else { None } },
                    { if !stderr.is_empty() { Some(stderr) } else { None } },
                ))
            } else {
                Err(std::io::Error::new(
                    std::io::ErrorKind::Unsupported,
                    ERR_UNKNOWN_EXITSTATUS,
                ))
            }
        }

        self.log(
            LogType::Debug,
            LOG_WHEN_START,
            LOG_STATUS_MSG,
            "checking command based condition",
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

        self.log(
            LogType::Debug,
            LOG_WHEN_START,
            LOG_STATUS_MSG,
            &format!("running command: `{}`", self.command_line()),
        );

        // run the process and capture possible errors
        let mut failure_reason: FailureReason = FailureReason::NoFailure;
        self._process_failed = false;
        self._process_status = 0;
        self._process_stderr = String::new();
        self._process_stdout = String::new();
        let startup_time = SystemTime::now();
        let open_process = Popen::create(&process_argv, process_config);
        if let Ok(process) = open_process {
            let proc_exit;

            match spawn_process(process, *DUR_SPAWNED_POLL_INTERVAL, self.timeout) {
                Ok((exit_status, out, err)) => {
                    if let Some(o) = out { self._process_stdout = o; }
                    if let Some(e) = err { self._process_stderr = e; }
                    proc_exit = Ok(exit_status);
                }
                Err(e) => {
                    match e.kind() {
                        std::io::ErrorKind::TimedOut => {
                            self.log(
                                LogType::Warn,
                                LOG_WHEN_PROC,
                                LOG_STATUS_FAIL,
                                &format!("timeout reached running command `{}`",self.command_line()),
                            );
                            proc_exit = Err(PopenError::from(std::io::Error::new(
                                ErrorKind::TimedOut,
                                ERR_TIMEOUT_REACHED,
                            )));
                        }
                        k => {
                            self.log(
                                LogType::Warn,
                                LOG_WHEN_PROC,
                                LOG_STATUS_FAIL,
                                &format!("error running command `{}`", self.command_line()),
                            );
                            proc_exit = Err(PopenError::from(
                                std::io::Error::new(k, e.to_string())));
                        }
                    }
                }
            }

            self._process_duration = SystemTime::now().duration_since(startup_time).unwrap();
            match proc_exit {
                Ok(exit_status) => {
                    let statusmsg: String;
                    if exit_status.success() {
                        // exit code is 0, and this usually indicates success
                        // however if it was not the expected exit code the
                        // failure reason has to be set to Status (for now);
                        // note that also the case of exit code 0 considered
                        // as a failure status is taken into account here
                        statusmsg = String::from("OK/0");
                        self.log(
                            LogType::Debug,
                            LOG_WHEN_PROC,
                            LOG_STATUS_OK,
                            &format!("command: `{}` exited with SUCCESS status {statusmsg}", self.command_line()),
                        );
                        self._process_status = 0;
                        if let Some(expected) = self.success_status {
                            if expected != 0 {
                                self.log(
                                    LogType::Debug,
                                    LOG_WHEN_PROC,
                                    LOG_STATUS_OK,
                                    &format!("condition expected success exit code NOT matched: {expected}"),
                                );
                                failure_reason = FailureReason::Status;
                            }
                        } else if let Some(expectedf) = self.failure_status {
                            if expectedf == 0 {
                                self.log(
                                    LogType::Debug,
                                    LOG_WHEN_PROC,
                                    LOG_STATUS_OK,
                                    &format!("condition expected failure exit code matched: {expectedf}"),
                                );
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
                                statusmsg = format!("ERROR/{v}");
                                self.log(
                                    LogType::Debug,
                                    LOG_WHEN_PROC,
                                    LOG_STATUS_OK,
                                    &format!("command: `{}` exited with FAILURE status {statusmsg}", self.command_line()),
                                );
                                if let Some(expectedf) = self.failure_status {
                                    if v == expectedf {
                                        self.log(
                                            LogType::Debug,
                                            LOG_WHEN_PROC,
                                            LOG_STATUS_OK,
                                            &format!("condition expected failure exit code {expectedf} matched"),
                                        );
                                        failure_reason = FailureReason::Status;
                                    } else if let Some(expected) = self.success_status {
                                        if v == expected {
                                            self.log(
                                                LogType::Debug,
                                                LOG_WHEN_PROC,
                                                LOG_STATUS_OK,
                                                &format!("condition expected success exit code {expected} matched"),
                                            );
                                        } else {
                                            self.log(
                                                LogType::Debug,
                                                LOG_WHEN_PROC,
                                                LOG_STATUS_OK,
                                                &format!("condition expected success exit code {expected} NOT matched: {v}"),
                                            );
                                            failure_reason = FailureReason::Status;
                                        }
                                    } else {
                                        self.log(
                                            LogType::Debug,
                                            LOG_WHEN_PROC,
                                            LOG_STATUS_OK,
                                            &format!("condition expected failure exit code {expectedf} NOT matched"),
                                        );
                                    }
                                } else if let Some(expected) = self.success_status {
                                    if v == expected {
                                        self.log(
                                            LogType::Debug,
                                            LOG_WHEN_PROC,
                                            LOG_STATUS_OK,
                                            &format!("condition expected success exit code {expected} matched"),
                                        );
                                    } else {
                                        self.log(
                                            LogType::Debug,
                                            LOG_WHEN_PROC,
                                            LOG_STATUS_OK,
                                            &format!("condition expected success exit code {expected} NOT matched: {v}"),
                                        );
                                        failure_reason = FailureReason::Status;
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
                                statusmsg = format!("SIGNAL/{v}");
                                self.log(
                                    LogType::Warn,
                                    LOG_WHEN_PROC,
                                    LOG_STATUS_FAIL,
                                    &format!("command: `{}` ended for reason {statusmsg}", self.command_line()),
                                );
                                failure_reason = FailureReason::Other;
                            }
                            ExitStatus::Other(v) => {
                                statusmsg = format!("UNKNOWN/{v}");
                                self.log(
                                    LogType::Warn,
                                    LOG_WHEN_PROC,
                                    LOG_STATUS_FAIL,
                                    &format!("command: `{}` ended for reason {statusmsg}", self.command_line()),
                                );
                                failure_reason = FailureReason::Other;
                            }
                            ExitStatus::Undetermined => {
                                statusmsg = format!("UNDETERMINED");
                                self.log(
                                    LogType::Warn,
                                    LOG_WHEN_PROC,
                                    LOG_STATUS_FAIL,
                                    &format!("command: `{}` ended for reason {statusmsg}", self.command_line()),
                                );
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
                                                    self.log(
                                                        LogType::Debug,
                                                        LOG_WHEN_PROC,
                                                        LOG_STATUS_OK,
                                                        &format!("condition success stdout (regex) {p:?} matched"),
                                                    );
                                                } else {
                                                    self.log(
                                                        LogType::Debug,
                                                        LOG_WHEN_PROC,
                                                        LOG_STATUS_OK,
                                                        &format!("condition success stdout (regex) {p:?} NOT matched"),
                                                    );
                                                    failure_reason = FailureReason::StdOut;
                                                }
                                            } else if re.find(&self._process_stdout).is_some() {
                                                self.log(
                                                    LogType::Debug,
                                                    LOG_WHEN_PROC,
                                                    LOG_STATUS_OK,
                                                    &format!("condition success stdout (regex) {p:?} found"),
                                                );
                                            } else {
                                                self.log(
                                                    LogType::Debug,
                                                    LOG_WHEN_PROC,
                                                    LOG_STATUS_OK,
                                                    &format!("condition success stdout (regex) {p:?} NOT found"),
                                                );
                                                failure_reason = FailureReason::StdOut;
                                            }
                                        } else {
                                            self.log(
                                                LogType::Error,
                                                LOG_WHEN_PROC,
                                                LOG_STATUS_FAIL,
                                                &format!("provided INVALID stdout regex {p:?} NOT found/matched"),
                                            );
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
                                                    self.log(
                                                        LogType::Debug,
                                                        LOG_WHEN_PROC,
                                                        LOG_STATUS_OK,
                                                        &format!("condition success stderr (regex) {p:?} matched"),
                                                    );
                                                } else {
                                                    self.log(
                                                        LogType::Debug,
                                                        LOG_WHEN_PROC,
                                                        LOG_STATUS_OK,
                                                        &format!("condition success stderr (regex) {p:?} NOT matched"),
                                                    );
                                                    failure_reason = FailureReason::StdErr;
                                                }
                                            } else if re.find(&self._process_stderr).is_some() {
                                                self.log(
                                                    LogType::Debug,
                                                    LOG_WHEN_PROC,
                                                    LOG_STATUS_OK,
                                                    &format!("condition success stderr (regex) {p:?} found"),
                                                );
                                            } else {
                                                self.log(
                                                    LogType::Debug,
                                                    LOG_WHEN_PROC,
                                                    LOG_STATUS_OK,
                                                    &format!("condition success stderr (regex) {p:?} NOT found"),
                                                );
                                                failure_reason = FailureReason::StdErr;
                                            }
                                        } else {
                                            self.log(
                                                LogType::Error,
                                                LOG_WHEN_PROC,
                                                LOG_STATUS_FAIL,
                                                &format!("provided INVALID stderr regex {p:?} NOT found/matched"),
                                            );
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
                                                    self.log(
                                                        LogType::Debug,
                                                        LOG_WHEN_PROC,
                                                        LOG_STATUS_OK,
                                                        &format!("condition failure stdout (regex) {p:?} matched"),
                                                    );
                                                    failure_reason = FailureReason::StdOut;
                                                } else {
                                                    self.log(
                                                        LogType::Debug,
                                                        LOG_WHEN_PROC,
                                                        LOG_STATUS_OK,
                                                        &format!("condition failure stdout (regex) {p:?} NOT matched"),
                                                    );
                                                }
                                            } else if re.find(&self._process_stdout).is_some() {
                                                self.log(
                                                    LogType::Debug,
                                                    LOG_WHEN_PROC,
                                                    LOG_STATUS_OK,
                                                    &format!("condition failure stdout (regex) {p:?} found"),
                                                );
                                                failure_reason = FailureReason::StdOut;
                                            } else {
                                                self.log(
                                                    LogType::Debug,
                                                    LOG_WHEN_PROC,
                                                    LOG_STATUS_OK,
                                                    &format!("condition failure stdout (regex) {p:?} NOT found"),
                                                );
                                            }
                                        } else {
                                            self.log(
                                                LogType::Error,
                                                LOG_WHEN_PROC,
                                                LOG_STATUS_FAIL,
                                                &format!("provided INVALID failure stdout regex {p:?} NOT found/matched"),
                                            );
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
                                                    self.log(
                                                        LogType::Debug,
                                                        LOG_WHEN_PROC,
                                                        LOG_STATUS_OK,
                                                        &format!("condition success stderr (regex) {p:?} matched"),
                                                    );
                                                    failure_reason = FailureReason::StdErr;
                                                } else {
                                                    self.log(
                                                        LogType::Debug,
                                                        LOG_WHEN_PROC,
                                                        LOG_STATUS_OK,
                                                        &format!("condition success stderr (regex) {p:?} NOT matched"),
                                                    );
                                                }
                                            } else if re.find(&self._process_stderr).is_some() {
                                                self.log(
                                                    LogType::Debug,
                                                    LOG_WHEN_PROC,
                                                    LOG_STATUS_OK,
                                                    &format!("condition success stderr (regex) {p:?} found"),
                                                );
                                                failure_reason = FailureReason::StdErr;
                                            } else {
                                                self.log(
                                                    LogType::Debug,
                                                    LOG_WHEN_PROC,
                                                    LOG_STATUS_OK,
                                                    &format!("condition success stderr (regex) {p:?} NOT found"),
                                                );
                                            }
                                        } else {
                                            self.log(
                                                LogType::Error,
                                                LOG_WHEN_PROC,
                                                LOG_STATUS_FAIL,
                                                &format!("provided INVALID stderr regex {p:?} NOT found/matched"),
                                            );
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
                                                    self.log(
                                                        LogType::Debug,
                                                        LOG_WHEN_PROC,
                                                        LOG_STATUS_OK,
                                                        &format!("condition success stdout {p:?} matched"),
                                                    );
                                                } else {
                                                    self.log(
                                                        LogType::Debug,
                                                        LOG_WHEN_PROC,
                                                        LOG_STATUS_OK,
                                                        &format!("condition success stdout {p:?} NOT matched"),
                                                    );
                                                    failure_reason = FailureReason::StdOut;
                                                }
                                            } else if self._process_stdout.contains(p) {
                                                self.log(
                                                    LogType::Debug,
                                                    LOG_WHEN_PROC,
                                                    LOG_STATUS_OK,
                                                    &format!("condition success stdout {p:?} found"),
                                                );
                                            } else {
                                                self.log(
                                                    LogType::Debug,
                                                    LOG_WHEN_PROC,
                                                    LOG_STATUS_OK,
                                                    &format!("condition success stdout {p:?} NOT found"),
                                                );
                                                failure_reason = FailureReason::StdOut;
                                            }
                                        }}
                                    }
                                    // B.2a CS text success stderr check
                                    if failure_reason == FailureReason::NoFailure {
                                        if let Some(p) = &self.success_stderr { if !p.is_empty() {
                                            if self.match_exact {
                                                if self._process_stderr == *p {
                                                    self.log(
                                                        LogType::Debug,
                                                        LOG_WHEN_PROC,
                                                        LOG_STATUS_OK,
                                                        &format!("condition success stderr {p:?} matched"),
                                                    );
                                                } else {
                                                    self.log(
                                                        LogType::Debug,
                                                        LOG_WHEN_PROC,
                                                        LOG_STATUS_OK,
                                                        &format!("condition success stderr {p:?} NOT matched"),
                                                    );
                                                    failure_reason = FailureReason::StdErr;
                                                }
                                            } else if self._process_stderr.contains(p) {
                                                self.log(
                                                    LogType::Debug,
                                                    LOG_WHEN_PROC,
                                                    LOG_STATUS_OK,
                                                    &format!("condition success stderr {p:?} found"),
                                                );
                                            } else {
                                                self.log(
                                                    LogType::Debug,
                                                    LOG_WHEN_PROC,
                                                    LOG_STATUS_OK,
                                                    &format!("condition success stderr {p:?} NOT found"),
                                                );
                                                failure_reason = FailureReason::StdErr;
                                            }
                                        }}
                                    }
                                    // B.3a CS text failure stdout check
                                    if failure_reason == FailureReason::NoFailure {
                                        if let Some(p) = &self.failure_stdout { if !p.is_empty() {
                                            if self.match_exact {
                                                if self._process_stdout == *p {
                                                    self.log(
                                                        LogType::Debug,
                                                        LOG_WHEN_PROC,
                                                        LOG_STATUS_OK,
                                                        &format!("condition failure stdout {p:?} matched"),
                                                    );
                                                    failure_reason = FailureReason::StdOut;
                                                } else {
                                                    self.log(
                                                        LogType::Debug,
                                                        LOG_WHEN_PROC,
                                                        LOG_STATUS_OK,
                                                        &format!("condition failure stdout {p:?} NOT matched"),
                                                    );
                                                }
                                            } else if self._process_stdout.contains(p) {
                                                self.log(
                                                    LogType::Debug,
                                                    LOG_WHEN_PROC,
                                                    LOG_STATUS_OK,
                                                    &format!("condition failure stdout {p:?} found"),
                                                );
                                                failure_reason = FailureReason::StdOut;
                                            } else {
                                                self.log(
                                                    LogType::Debug,
                                                    LOG_WHEN_PROC,
                                                    LOG_STATUS_OK,
                                                    &format!("condition failure stdout {p:?} NOT found"),
                                                );
                                            }
                                        }}
                                    }
                                    // B.4a CS text failure stderr check
                                    if failure_reason == FailureReason::NoFailure {
                                        if let Some(p) = &self.failure_stderr { if !p.is_empty() {
                                            if self.match_exact {
                                                if self._process_stderr == *p {
                                                    self.log(
                                                        LogType::Debug,
                                                        LOG_WHEN_PROC,
                                                        LOG_STATUS_OK,
                                                        &format!("condition failure stderr {p:?} matched"),
                                                    );
                                                    failure_reason = FailureReason::StdErr;
                                                } else {
                                                    self.log(
                                                        LogType::Debug,
                                                        LOG_WHEN_PROC,
                                                        LOG_STATUS_OK,
                                                        &format!("condition failure stderr {p:?} NOT matched"),
                                                    );
                                                }
                                            } else if self._process_stderr.contains(p) {
                                                self.log(
                                                    LogType::Debug,
                                                    LOG_WHEN_PROC,
                                                    LOG_STATUS_OK,
                                                    &format!("condition failure stderr {p:?} found"),
                                                );
                                                failure_reason = FailureReason::StdErr;
                                            } else {
                                                self.log(
                                                    LogType::Debug,
                                                    LOG_WHEN_PROC,
                                                    LOG_STATUS_OK,
                                                    &format!("condition failure stderr {p:?} NOT found"),
                                                );
                                            }
                                        }}
                                    }
                                } else {
                                    // B.1b CI text success stdout check
                                    if failure_reason == FailureReason::NoFailure {
                                        if let Some(p) = &self.success_stdout { if !p.is_empty() {
                                            if self.match_exact {
                                                if self._process_stdout.to_uppercase() == p.to_uppercase() {
                                                    self.log(
                                                        LogType::Debug,
                                                        LOG_WHEN_PROC,
                                                        LOG_STATUS_OK,
                                                        &format!("condition success stdout {p:?} matched"),
                                                    );
                                                } else {
                                                    self.log(
                                                        LogType::Debug,
                                                        LOG_WHEN_PROC,
                                                        LOG_STATUS_OK,
                                                        &format!("condition success stdout {p:?} NOT matched"),
                                                    );
                                                    failure_reason = FailureReason::StdOut;
                                                }
                                            } else if self._process_stdout.to_uppercase().contains(&p.to_uppercase()) {
                                                self.log(
                                                    LogType::Debug,
                                                    LOG_WHEN_PROC,
                                                    LOG_STATUS_OK,
                                                    &format!("condition success stdout {p:?} found"),
                                                );
                                            } else {
                                                self.log(
                                                    LogType::Debug,
                                                    LOG_WHEN_PROC,
                                                    LOG_STATUS_OK,
                                                    &format!("condition success stdout {p:?} NOT found"),
                                                );
                                                failure_reason = FailureReason::StdOut;
                                            }
                                        }}
                                    }
                                    // B.2b CI text success stderr check
                                    if failure_reason == FailureReason::NoFailure {
                                        if let Some(p) = &self.success_stderr { if !p.is_empty() {
                                            if self.match_exact {
                                                if self._process_stderr.to_uppercase() == p.to_uppercase() {
                                                    self.log(
                                                        LogType::Debug,
                                                        LOG_WHEN_PROC,
                                                        LOG_STATUS_OK,
                                                        &format!("condition success stderr {p:?} matched"),
                                                    );
                                                } else {
                                                    self.log(
                                                        LogType::Debug,
                                                        LOG_WHEN_PROC,
                                                        LOG_STATUS_OK,
                                                        &format!("condition success stderr {p:?} NOT matched"),
                                                    );
                                                    failure_reason = FailureReason::StdErr;
                                                }
                                            } else if self._process_stderr.to_uppercase().contains(&p.to_uppercase()) {
                                                self.log(
                                                    LogType::Debug,
                                                    LOG_WHEN_PROC,
                                                    LOG_STATUS_OK,
                                                    &format!("condition success stderr {p:?} found"),
                                                );
                                            } else {
                                                self.log(
                                                    LogType::Debug,
                                                    LOG_WHEN_PROC,
                                                    LOG_STATUS_OK,
                                                    &format!("condition success stderr {p:?} NOT found"),
                                                );
                                                failure_reason = FailureReason::StdErr;
                                            }
                                        }}
                                    }
                                    // B.3b CI text failure stdout check
                                    if failure_reason == FailureReason::NoFailure {
                                        if let Some(p) = &self.failure_stdout { if !p.is_empty() {
                                            if self.match_exact {
                                                if self._process_stdout.to_uppercase() == p.to_uppercase() {
                                                    self.log(
                                                        LogType::Debug,
                                                        LOG_WHEN_PROC,
                                                        LOG_STATUS_OK,
                                                        &format!("condition failure stdout {p:?} matched"),
                                                    );
                                                    failure_reason = FailureReason::StdOut;
                                                } else {
                                                    self.log(
                                                        LogType::Debug,
                                                        LOG_WHEN_PROC,
                                                        LOG_STATUS_OK,
                                                        &format!("condition failure stdout {p:?} NOT matched"),
                                                    );
                                                }
                                            } else if self._process_stdout.to_uppercase().contains(&p.to_uppercase()) {
                                                self.log(
                                                    LogType::Debug,
                                                    LOG_WHEN_PROC,
                                                    LOG_STATUS_OK,
                                                    &format!("condition failure stdout {p:?} found"),
                                                );
                                                failure_reason = FailureReason::StdOut;
                                            } else {
                                                self.log(
                                                    LogType::Debug,
                                                    LOG_WHEN_PROC,
                                                    LOG_STATUS_OK,
                                                    &format!("condition failure stdout {p:?} NOT found"),
                                                );
                                            }
                                        }}
                                    }
                                    // B.4b CI text failure stderr check
                                    if failure_reason == FailureReason::NoFailure {
                                        if let Some(p) = &self.failure_stderr { if !p.is_empty() {
                                            if self.match_exact {
                                                if self._process_stderr.to_uppercase() == p.to_uppercase() {
                                                    self.log(
                                                        LogType::Debug,
                                                        LOG_WHEN_PROC,
                                                        LOG_STATUS_OK,
                                                        &format!("condition failure stderr {p:?} matched"),
                                                    );
                                                    failure_reason = FailureReason::StdErr;
                                                } else {
                                                    self.log(
                                                        LogType::Debug,
                                                        LOG_WHEN_PROC,
                                                        LOG_STATUS_OK,
                                                        &format!("condition failure stderr {p:?} NOT matched"),
                                                    );
                                                }
                                            } else if self._process_stderr.to_uppercase().contains(&p.to_uppercase()) {
                                                self.log(
                                                    LogType::Debug,
                                                    LOG_WHEN_PROC,
                                                    LOG_STATUS_OK,
                                                    &format!("condition failure stderr {p:?} found"),
                                                );
                                                failure_reason = FailureReason::StdErr;
                                            } else {
                                                self.log(
                                                    LogType::Debug,
                                                    LOG_WHEN_PROC,
                                                    LOG_STATUS_OK,
                                                    &format!("condition failure stderr {p:?} NOT found"),
                                                );
                                            }
                                        }}
                                    }
                                }
                            }
                        }
                        _ => {
                            // need not to check for other failure types
                            self._process_failed = true;
                        }
                    }
                }

                // the command could not be executed thus an error is reported
                Err(e) => {
                    self.log(
                        LogType::Warn,
                        LOG_WHEN_END,
                        LOG_STATUS_FAIL,
                        &format!("could not execute command: `{}` (reason: {e})", self.command_line()),
                    );
                    self._process_failed = true;
                    failure_reason = FailureReason::Other;
                }
            }
        } else {
            // something happened before the command could be run
            if let Err(e) = open_process {
                self._process_failed = true;
                self.log(
                    LogType::Warn,
                    LOG_WHEN_END,
                    LOG_STATUS_FAIL,
                    &format!("could not start command: `{}` (reason: {e})", self.command_line()),
                );
            } else {
                self._process_failed = true;
                self.log(
                    LogType::Warn,
                    LOG_WHEN_END,
                    LOG_STATUS_FAIL,
                    &format!("could not start command: `{}` (reason: unknown)", self.command_line()),
                );
            }
            failure_reason = FailureReason::Other;
        }

        // now the time of the last check can be set to the actual time in
        // order to allow further checks to comply with the request to be
        // only run at certain intervals
        self.check_last = t;

        // return true on success and false otherwise
        match failure_reason {
            FailureReason::NoFailure => {
                self.log(
                    LogType::Debug,
                    LOG_WHEN_END,
                    LOG_STATUS_OK,
                    &format!("condition checked successfully in {:.2}s", self._process_duration.as_secs_f64()),
                );
                Ok(Some(true))
            }
            FailureReason::StdOut => {
                self._process_failed = true;
                self.log(
                    LogType::Info,
                    LOG_WHEN_END,
                    LOG_STATUS_FAIL,
                    &format!("condition checked unsuccessfully (stdout check) in {:.2}s", self._process_duration.as_secs_f64()),
                );
                Ok(Some(false))
            }
            FailureReason::StdErr => {
                self._process_failed = true;
                self.log(
                    LogType::Info,
                    LOG_WHEN_END,
                    LOG_STATUS_FAIL,
                    &format!("condition checked unsuccessfully (stderr check) in {:.2}s",self._process_duration.as_secs_f64()),
                );
                Ok(Some(false))
            }
            FailureReason::Status => {
                self._process_failed = true;
                self.log(
                    LogType::Info,
                    LOG_WHEN_END,
                    LOG_STATUS_FAIL,
                    &format!("condition checked unsuccessfully (status check) in {:.2}s", self._process_duration.as_secs_f64()),
                );
                Ok(Some(false))
            }
            FailureReason::Other => {
                self._process_failed = true;
                self.log(
                    LogType::Info,
                    LOG_WHEN_END,
                    LOG_STATUS_FAIL,
                    &format!("task ended unexpectedly in {:.2}s", self._process_duration.as_secs_f64()),
                );
                Ok(Some(false))
            }
        }
    }

}


// end.

