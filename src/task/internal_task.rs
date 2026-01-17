//! Define a task based on internal input commands
//!
//! This type of task simply takes a command line as it would be sent to the
//! application via _stdin_ (normally by a wrapper) and calls the same routine
//! that executes such command lines and tries to run it. All commands are
//! theoretically supported: the application is therefore able to kill itself
//! or, for instance, to pause itself so that only the user intervention can
//! wake it up (once paused, a `resume` command cannot be run unattended).

use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Mutex;
use std::time::SystemTime;

use cfgmap::CfgMap;
use lazy_static::lazy_static;

// we implement the Task trait here in order to enqueue tasks
use super::base::Task;
use crate::common::logging::{LogType, log};
use crate::common::wres::Result;
use crate::{cfg_mandatory, constants::*};

use crate::cfghelp::*;

// a command runner function type
type CommandRunnerFunction = fn(&str) -> Result<bool>;

// and the command runner structure to be instantiated only once
struct CommandRunner {
    command_runner: Option<CommandRunnerFunction>,
}

impl CommandRunner {
    pub fn new() -> Self {
        CommandRunner {
            command_runner: None,
        }
    }

    pub fn set_runner(&mut self, f: CommandRunnerFunction) {
        self.command_runner = Some(f);
    }
}

// an instance of the command runner that will be used by all tasks to run
// internal commands: it is synchronized in order to avoid collisions
lazy_static! {
    static ref COMMAND_RUNNER: Mutex<CommandRunner> = Mutex::new(CommandRunner::new());
}

// this setter is accessible from outside
#[allow(dead_code)]
pub fn set_command_runner(f: CommandRunnerFunction) -> Result<()> {
    let mut runner = COMMAND_RUNNER.lock()?;
    runner.set_runner(f);
    Ok(())
}

/// Internal Task
///
/// A task that executes an internal command.
pub struct InternalTask {
    // common members
    task_id: i64,
    task_name: String,

    // specific members
    // parameters
    command: String,
    // internal values
    // (none here)
}

// implement the hash protocol
impl Hash for InternalTask {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.task_name.hash(state);
        self.command.hash(state);
    }
}

#[allow(dead_code)]
impl InternalTask {
    /// Create a new internal input command based task
    ///
    /// The only parameters that have to be set mandatorily upon creation of
    /// an internal input command based task are the following.
    ///
    /// # Arguments
    ///
    /// * `name` - a string containing the name of the task
    /// * `command` - an internal command as described in the README file
    ///
    /// No other arguments are needed, and the configuration of the item
    /// only allows to specify the command.
    pub fn new(name: &str, command: &str) -> Self {
        log(
            LogType::Debug,
            LOG_EMITTER_TASK_INTERNAL,
            LOG_ACTION_NEW,
            Some((name, 0)),
            LOG_WHEN_INIT,
            LOG_STATUS_MSG,
            &format!("TASK {name}: creating a new internal command based task"),
        );
        Self {
            task_id: 0,
            task_name: String::from(name),
            command: String::from(command),
        }
    }

    /// Load an `InternalTask` from a [`CfgMap`](https://docs.rs/cfgmap/latest/)
    ///
    /// The `InternalTask` is initialized according to the values provided in
    /// the `CfgMap` argument. If the `CfgMap` format does not comply with
    /// the requirements of a `InternalTask` an error is raised.
    pub fn load_cfgmap(cfgmap: &CfgMap) -> Result<InternalTask> {
        let check = vec!["type", "name", "command"];
        cfg_check_keys(cfgmap, &check)?;

        // type and name are both mandatory but type is only checked
        cfg_mandatory!(cfg_string_check_exact(cfgmap, "type", "internal"))?;
        let name = cfg_mandatory!(cfg_string_check_regex(cfgmap, "name", &RE_TASK_NAME))?.unwrap();

        // specific mandatory parameter retrieval
        let command = cfg_mandatory!(cfg_string(cfgmap, "command"))?.unwrap();

        // initialize the structure
        let new_task = InternalTask::new(&name, &command);

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
        // (none here)

        Ok(new_task)
    }

    /// Check a configuration map and return item name if Ok
    ///
    /// The check is performed exactly in the same way and in the same order
    /// as in `load_cfgmap`, the only difference is that no actual item is
    /// created and that a name is returned, which is the name of the item that
    /// _would_ be created via the equivalent call to `load_cfgmap`
    pub fn check_cfgmap(cfgmap: &CfgMap) -> Result<String> {
        let check = vec!["type", "name", "command"];
        cfg_check_keys(cfgmap, &check)?;

        // type and name are both mandatory but type is only checked
        cfg_mandatory!(cfg_string_check_exact(cfgmap, "type", "internal"))?;
        let name = cfg_mandatory!(cfg_string_check_regex(cfgmap, "name", &RE_TASK_NAME))?.unwrap();

        // specific mandatory parameter retrieval
        cfg_mandatory!(cfg_string(cfgmap, "command"))?;

        Ok(name)
    }
}

impl Task for InternalTask {
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

    /// Execute this `LuaTask`
    ///
    /// This implementation of the trait `run()` function obeys to the main
    /// trait's constraints, and returns an error if any error happened while
    /// running the internal command, Ok(false) if there were no errors but
    /// the command could not be run, Ok(true) on success.
    fn _run(&mut self, trigger_name: &str) -> Result<Option<bool>> {
        // the None case would simply be a mistake, therefore the assertion
        let cr = COMMAND_RUNNER.lock()?;
        assert!(cr.command_runner.is_some(), "command runner not set");

        // log the beginning
        let runner = cr.command_runner.unwrap();
        self.log(
            LogType::Debug,
            LOG_WHEN_START,
            LOG_STATUS_OK,
            &format!(
                "(trigger: {trigger_name}) executing internal command: `{}`",
                self.command,
            ),
        );

        // start execution
        let startup_time = SystemTime::now();

        // run the command
        let res = runner(&self.command);

        // log the final message and return the condition outcome
        let duration = SystemTime::now().duration_since(startup_time).unwrap();

        if let Ok(r) = res {
            if r {
                self.log(
                    LogType::Trace,
                    LOG_WHEN_END,
                    LOG_STATUS_OK,
                    &format!(
                        "(trigger: {trigger_name}) internal command `{}` executed successfully in {:.2}s",
                        self.command,
                        duration.as_secs_f64(),
                    ),
                );
                Ok(Some(true))
            } else {
                self.log(
                    LogType::Debug,
                    LOG_WHEN_END,
                    LOG_STATUS_FAIL,
                    &format!(
                        "(trigger: {trigger_name}) internal command `{}` could not be executed",
                        self.command,
                    ),
                );
                Ok(Some(false))
            }
        } else {
            let e = res.unwrap_err();
            self.log(
                LogType::Warn,
                LOG_WHEN_END,
                LOG_STATUS_ERR,
                &format!(
                    "(trigger: {trigger_name}) internal command `{}` exited with error in {:.2}s: {e}",
                    self.command,
                    duration.as_secs_f64(),
                ),
            );
            Err(e)
        }
    }
}

// end.
