//! Define an OS command based task
//!
//! This is actually the most common type of `Task` concrete implementation,
//! designed to execute OS processes. A process is started by the task by
//! invoking the related OS command (that is, starting an executable) and
//! retrieving its outcome in the form of exit codes and messages written to
//! either _stdout_ and/or _stderr_. A command based task is constructed so
//! that checking one or more parts of its outcome determine the final status
//! of success or failure. Such checks consist in:
//!
//! * testing _stdout_ and/or _stderr_ for the presence of a particular
//!   string or against a regular expression
//! * matching the exit code with a given value
//!
//! The operations are thoroughly logged and the final status is returned.
//!
//! A command based task can, if requested to, modify the environment in which
//! the OS command is executed, by adding environment variables or even by
//! avoiding to pass the existing environment to the spawned process. In
//! addition, the task can add environment variables describing the task name
//! and the name of the condition that triggered it, for commands (for example
//! scripts) that could be aware of being invoked by a task.

use std::collections::HashMap;
use std::env;
use std::ffi::OsString;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::ErrorKind;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use itertools::Itertools;

use subprocess::{Popen, PopenConfig, PopenError, Redirection};

use cfgmap::CfgMap;

// we implement the Task trait here in order to enqueue tasks
use super::base::Task;
use crate::common::cmditem::*;
use crate::common::logging::{LogType, log};
use crate::common::wres::Result;
use crate::{cfg_mandatory, constants::*};

use crate::cfghelp::*;

/// Command Based Task
///
/// This type of task invokes an OS command and checks its outcome by examining
/// the exit code and/or the contents of _stdout_ and _stderr_.
pub struct CommandTask {
    // common members
    task_id: i64,
    task_name: String,

    // specific members
    // parameters
    command: PathBuf,
    args: Vec<String>,
    include_env: bool,
    match_exact: bool,
    match_regexp: bool,
    case_sensitive: bool,
    set_envvars: bool,
    environment_vars: HashMap<String, String>,

    // internal values
    success_stdout: Option<String>,
    success_stderr: Option<String>,
    success_status: Option<u32>,
    failure_stdout: Option<String>,
    failure_stderr: Option<String>,
    failure_status: Option<u32>,
    timeout: Option<Duration>,
    startup_dir: PathBuf,
    _process_stdout: String,
    _process_stderr: String,
    _process_status: u32,
    _process_failed: bool,
    _process_duration: Duration,
}

// implement the hash protocol
impl Hash for CommandTask {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.task_name.hash(state);
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
impl CommandTask {
    /// Create a new command based task
    ///
    /// The only parameters that have to be set mandatorily upon creation of a
    /// command based task are the following.
    ///
    /// # Arguments
    ///
    /// * `name` - a string containing the name of the task
    /// * `command` - the full path to an OS executable
    /// * `args` - a list of parameters to be passed to the command
    /// * `startup_dir` - the OS directory where to start the command from
    ///
    /// The `command` and `startup_dir` are `PathBuf`s, and must exist. The
    /// command arguments must be passed in a `Vec` of owned `String`s in order
    /// to enable the correct invocation.
    ///
    /// FIXME:
    ///     1. use a `Vec<&str>` instead of a `Vec<String>` for arguments
    ///     2. use `Path` for `command` and `startup_dir`.
    pub fn new(name: &str, command: &PathBuf, args: &Vec<String>, startup_dir: &PathBuf) -> Self {
        log(
            LogType::Debug,
            LOG_EMITTER_TASK_COMMAND,
            LOG_ACTION_NEW,
            Some((name, 0)),
            LOG_WHEN_INIT,
            LOG_STATUS_MSG,
            &format!("TASK {name}: creating a new command based task"),
        );
        CommandTask {
            // common members initialization
            // reset ID to zero
            task_id: 0,

            // parameters
            task_name: String::from(name),

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
            success_stdout: None,
            success_stderr: None,
            success_status: None,
            failure_stdout: None,
            failure_stderr: None,
            failure_status: None,
            timeout: None,

            // internal values
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

    /// Set a variable in the custom environment that the command base task
    /// provides to the spawned process.
    ///
    /// # Arguments
    ///
    /// * `var` - the variable name
    /// * `value` - the value assigned to the named variable
    pub fn set_variable(&mut self, var: &str, value: &str) -> Option<String> {
        self.environment_vars
            .insert(String::from(var), String::from(value))
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

    /// Load a `CommandTask` from a [`CfgMap`](https://docs.rs/cfgmap/latest/)
    ///
    /// The `CommandTask` is initialized according to the values provided in
    /// the `CfgMap` argument. If the `CfgMap` format does not comply with
    /// the requirements of a `CommandTask` an error is raised.
    pub fn load_cfgmap(cfgmap: &CfgMap) -> Result<CommandTask> {
        let check = vec![
            "type",
            "name",
            "tags",
            "command",
            "command_arguments",
            "startup_path",
            "match_exact",
            "match_regular_expression",
            "case_sensitive",
            "include_environment",
            "set_environment_variables",
            "environment_variables",
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
        let name = cfg_mandatory!(cfg_string_check_regex(cfgmap, "name", &RE_TASK_NAME))?.unwrap();

        // specific mandatory parameter retrieval
        let command = PathBuf::from(cfg_mandatory!(cfg_string(cfgmap, "command"))?.unwrap());
        let args = cfg_mandatory!(cfg_vec_string(cfgmap, "command_arguments"))?.unwrap();
        let startup_path =
            PathBuf::from(cfg_mandatory!(cfg_string(cfgmap, "startup_path"))?.unwrap());
        if !startup_path.is_dir() {
            return Err(cfg_err_invalid_config(
                "startup_path",
                &startup_path.to_string_lossy(),
                ERR_INVALID_STARTUP_PATH,
            ));
        };

        // initialize the structure
        let mut new_task = CommandTask::new(&name, &command, &args, &startup_path);

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
        if let Some(v) = cfg_bool(cfgmap, "match_exact")? {
            new_task.match_exact = v;
        }
        if let Some(v) = cfg_bool(cfgmap, "match_regular_expression")? {
            new_task.match_regexp = v;
        }
        if let Some(v) = cfg_bool(cfgmap, "case_sensitive")? {
            new_task.case_sensitive = v;
        }
        if let Some(v) = cfg_bool(cfgmap, "include_environment")? {
            new_task.include_env = v;
        }
        if let Some(v) = cfg_bool(cfgmap, "set_environment_variables")? {
            new_task.set_envvars = v;
        }

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
                            name,
                            ERR_INVALID_ENVVAR_NAME,
                        ));
                    } else if let Some(value) = map.get(name) {
                        if value.is_str()
                            || value.is_int()
                            || value.is_float()
                            || value.is_datetime()
                        {
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
                new_task.environment_vars = vars;
            }
        }

        new_task.success_stdout = cfg_string(cfgmap, "success_stdout")?;
        new_task.success_stderr = cfg_string(cfgmap, "success_stderr")?;
        if let Some(v) = cfg_int_check_interval(cfgmap, "success_status", 0, std::u32::MAX as i64)?
        {
            new_task.success_status = Some(v as u32);
        }

        new_task.failure_stdout = cfg_string(cfgmap, "failure_stdout")?;
        new_task.failure_stderr = cfg_string(cfgmap, "failure_stderr")?;
        if let Some(v) = cfg_int_check_interval(cfgmap, "failure_status", 0, std::u32::MAX as i64)?
        {
            new_task.failure_status = Some(v as u32);
        }

        if let Some(v) = cfg_int_check_above_eq(cfgmap, "timeout_seconds", 0)? {
            if v > 0 {
                new_task.timeout = Some(Duration::from_secs(v as u64));
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
            "command",
            "command_arguments",
            "startup_path",
            "match_exact",
            "match_regular_expression",
            "case_sensitive",
            "include_environment",
            "set_environment_variables",
            "environment_variables",
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
        let name = cfg_mandatory!(cfg_string_check_regex(cfgmap, "name", &RE_TASK_NAME))?.unwrap();

        // specific mandatory parameter check
        cfg_mandatory!(cfg_string(cfgmap, "command"))?;
        cfg_mandatory!(cfg_vec_string(cfgmap, "command_arguments"))?;
        let startup_path =
            PathBuf::from(cfg_mandatory!(cfg_string(cfgmap, "startup_path"))?.unwrap());
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
                            name,
                            ERR_INVALID_ENVVAR_NAME,
                        ));
                    } else if let Some(value) = map.get(name) {
                        if value.is_str()
                            || value.is_int()
                            || value.is_float()
                            || value.is_datetime()
                        {
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

// implement the Task trait
impl Task for CommandTask {
    fn set_id(&mut self, id: i64) {
        self.task_id = id;
    }
    fn get_name(&self) -> String {
        self.task_name.clone()
    }
    fn get_id(&self) -> i64 {
        self.task_id
    }

    /// Return a hash of this item for comparison
    fn _hash(&self) -> u64 {
        let mut s = DefaultHasher::new();
        self.hash(&mut s);
        s.finish()
    }

    /// Execute this `CommandTask`
    ///
    /// This implementation of the trait `run()` function obeys to the main
    /// trait's constraints, and returns
    ///
    /// * `Ok(Some(true))` on success
    /// * `Ok(Some(false))` on failure
    /// * `Ok(None)` when the spawned process isn't checked for success
    /// * `Err(_)` if an error occurred
    ///
    /// The `Err(_)` result is an error condition usually determined by errors
    /// _prior_ to attempting to execute the OS process, as possible errors
    /// reported by the process itself result in values that are checked
    /// against values provided upon construction in the `expects`/`rejects`
    /// modifiers.
    fn _run(&mut self, trigger_name: &str) -> Result<Option<bool>> {
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
            temp_env.insert(ENVVAR_NAME_COND.to_string(), String::from(trigger_name));
            temp_env.insert(ENVVAR_NAME_TASK.to_string(), self.task_name.clone());
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
            &format!(
                "(trigger: {trigger_name}) running command: `{}`",
                self.command_line()
            ),
        );

        // run the process and capture possible errors
        // let mut failure_reason: FailureReason = FailureReason::NoFailure;
        let failure_reason;
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
                    if let Some(o) = out {
                        self._process_stdout = o;
                    }
                    if let Some(e) = err {
                        self._process_stderr = e;
                    }
                    proc_exit = Ok(exit_status);
                }
                Err(e) => match e.kind() {
                    std::io::ErrorKind::TimedOut => {
                        self.log(
                            LogType::Debug,
                            LOG_WHEN_PROC,
                            LOG_STATUS_FAIL,
                            &format!("timeout reached running command `{}`", self.command_line()),
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
                        proc_exit = Err(PopenError::from(std::io::Error::new(k, e.to_string())));
                    }
                },
            }

            self._process_duration = SystemTime::now().duration_since(startup_time).unwrap();
            match proc_exit {
                Ok(exit_status) => {
                    let ck_process_status;
                    let ck_process_failed;
                    let ck_failure_reason;
                    let log_severity;
                    let log_when;
                    let log_status;
                    let log_message;
                    (
                        ck_process_status,
                        ck_process_failed,
                        ck_failure_reason,
                        log_severity,
                        log_when,
                        log_status,
                        log_message,
                    ) = check_process_outcome(
                        &exit_status,
                        &self._process_stdout,
                        &self._process_stderr,
                        &self.command_line(),
                        self.match_exact,
                        self.match_regexp,
                        self.case_sensitive,
                        &self.success_stdout,
                        &self.success_stderr,
                        &self.success_status,
                        &self.failure_stdout,
                        &self.failure_stderr,
                        &self.failure_status,
                    );

                    self._process_status = ck_process_status;
                    self._process_failed = ck_process_failed;
                    failure_reason = ck_failure_reason;

                    self.log(
                        log_severity,
                        log_when,
                        log_status,
                        &format!("(trigger: {trigger_name}) {log_message}"),
                    );
                }
                // the command could not be executed thus an error is reported
                Err(e) => {
                    self.log(
                        LogType::Warn,
                        LOG_WHEN_END,
                        LOG_STATUS_FAIL,
                        &format!(
                            "(trigger: {trigger_name}) could not execute command: `{}` (reason: {})",
                            self.command_line(), e),
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
                    &format!(
                        "(trigger: {trigger_name}) could not start command: `{}` (reason: {e})",
                        self.command_line()
                    ),
                );
            } else {
                self._process_failed = true;
                self.log(
                    LogType::Warn,
                    LOG_WHEN_END,
                    LOG_STATUS_FAIL,
                    &format!(
                        "(trigger: {trigger_name}) could not start command: `{}` (reason: unknown)",
                        self.command_line()
                    ),
                );
            }
            failure_reason = FailureReason::Other;
        }

        // return true on success of false otherwise
        match failure_reason {
            FailureReason::NoFailure => {
                self.log(
                    LogType::Debug,
                    LOG_WHEN_END,
                    LOG_STATUS_OK,
                    &format!(
                        "(trigger: {trigger_name}) task exited successfully in {:.2}s",
                        self._process_duration.as_secs_f64()
                    ),
                );
                Ok(Some(true))
            }
            FailureReason::StdOut => {
                self._process_failed = true;
                self.log(
                    LogType::Debug,
                    LOG_WHEN_END,
                    LOG_STATUS_OK,
                    &format!(
                        "(trigger: {trigger_name}) task exited unsuccessfully (stdout check) in {:.2}s",
                        self._process_duration.as_secs_f64()),
                    );
                Ok(Some(false))
            }
            FailureReason::StdErr => {
                self._process_failed = true;
                self.log(
                    LogType::Debug,
                    LOG_WHEN_END,
                    LOG_STATUS_OK,
                    &format!(
                        "(trigger: {trigger_name}) task exited unsuccessfully (stderr check) in {:.2}s",
                        self._process_duration.as_secs_f64()),
                    );
                Ok(Some(false))
            }
            FailureReason::Status => {
                self._process_failed = true;
                self.log(
                    LogType::Debug,
                    LOG_WHEN_END,
                    LOG_STATUS_OK,
                    &format!(
                        "(trigger: {trigger_name}) task exited unsuccessfully (status check) in {:.2}s",
                        self._process_duration.as_secs_f64()),
                    );
                Ok(Some(false))
            }
            FailureReason::Other => {
                self._process_failed = true;
                self.log(
                    LogType::Warn,
                    LOG_WHEN_END,
                    LOG_STATUS_FAIL,
                    &format!(
                        "(trigger: {trigger_name}) command ended unexpectedly in {:.2}s",
                        self._process_duration.as_secs_f64()
                    ),
                );
                Ok(Some(false))
            }
        }
    }
}

// end.
