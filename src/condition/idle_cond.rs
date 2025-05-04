//! Define a user idle time based condition
//!
//! This type of condition is verified when the user has been idle for the
//! specified number of seconds: once verified, it will not recur unless after
//! the user has been active and then idle again for the same amount of seconds
//! (if its recurring state is set to `true`).

use std::hash::{DefaultHasher, Hash, Hasher};
use std::time::{Duration, Instant};

use cfgmap::CfgMap;
use user_idle::UserIdle;

use super::base::Condition;
use crate::common::logging::{LogType, log};
use crate::common::wres::Result;
use crate::task::registry::TaskRegistry;
use crate::{cfg_mandatory, constants::*};

use crate::cfghelp::*;

/// Idle Session Based Condition
///
/// This condition is verified once enough time has passed since without
/// user interaction with the workstation.
pub struct IdleCondition {
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
    idle_seconds: Duration,

    // internal values
    idle_verified: bool,
}

// implement the hash protocol
impl Hash for IdleCondition {
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
        self.idle_seconds.hash(state);
    }
}

#[allow(dead_code)]
impl IdleCondition {
    /// Create a new idle time based condition with the given name and idle
    /// time duration
    pub fn new(name: &str, interval: &Duration) -> Self {
        log(
            LogType::Debug,
            LOG_EMITTER_CONDITION_IDLE,
            LOG_ACTION_NEW,
            Some((name, 0)),
            LOG_WHEN_INIT,
            LOG_STATUS_MSG,
            &format!("CONDITION {name}: creating a new idle time based condition"),
        );
        IdleCondition {
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
            idle_seconds: *interval,

            // specific members initialization
            idle_verified: false,
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

    /// Load an `IdleCondition` from a [`CfgMap`](https://docs.rs/cfgmap/latest/)
    ///
    /// The `IntervalCondition` is initialized according to the values provided
    /// in the `CfgMap` argument. If the `CfgMap` format does not comply with
    /// the requirements of a `IdleCondition` an error is raised.
    pub fn load_cfgmap(
        cfgmap: &CfgMap,
        task_registry: &'static TaskRegistry,
    ) -> Result<IdleCondition> {
        let check = vec![
            "type",
            "name",
            "tags",
            "idle_seconds",
            "tasks",
            "recurring",
            "max_tasks_retries",
            "execute_sequence",
            "break_on_failure",
            "break_on_success",
            "suspended",
        ];
        cfg_check_keys(cfgmap, &check)?;

        // common mandatory parameter retrieval

        // type and name are both mandatory but type is only checked
        cfg_mandatory!(cfg_string_check_exact(cfgmap, "type", "idle"))?;
        let name = cfg_mandatory!(cfg_string_check_regex(cfgmap, "name", &RE_COND_NAME))?.unwrap();

        // specific mandatory parameter retrieval
        let idle_seconds = Duration::from_secs(
            cfg_mandatory!(cfg_int_check_above_eq(cfgmap, "idle_seconds", 0))?.unwrap() as u64,
        );

        // initialize the structure
        let mut new_condition = IdleCondition::new(&name, &idle_seconds);
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
        // (none here)

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
            "idle_seconds",
            "tasks",
            "recurring",
            "max_tasks_retries",
            "execute_sequence",
            "break_on_failure",
            "break_on_success",
            "suspended",
        ];
        cfg_check_keys(cfgmap, &check)?;

        // common mandatory parameter check

        // type and name are both mandatory: type is checked and name is kept
        cfg_mandatory!(cfg_string_check_exact(cfgmap, "type", "idle"))?;
        let name = cfg_mandatory!(cfg_string_check_regex(cfgmap, "name", &RE_EVENT_NAME))?.unwrap();

        cfg_mandatory!(cfg_int_check_above_eq(cfgmap, "idle_seconds", 0))?;

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

        Ok(name)
    }
}

impl Condition for IdleCondition {
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
        "idle"
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
    /// This function actually performs the test; check for the idle time to
    /// be longer than the configured idle time, and if so return `true` after
    /// setting internal state to stop returning `true` until idle time goes
    /// below the configured interval. In that case reset the internal state
    /// to start over checking (if _recurring_).
    fn _check_condition(&mut self) -> Result<Option<bool>> {
        if let Ok(idle) = UserIdle::get_time() {
            // last_tested has already been set by trait to Instant::now()
            self.log(
                LogType::Debug,
                LOG_WHEN_PROC,
                LOG_STATUS_MSG,
                &format!(
                    "checking idle time based condition{} (test: {}<{}?)",
                    { if self.idle_verified { " [idle]" } else { "" } },
                    idle.as_seconds(),
                    self.idle_seconds.as_secs(),
                ),
            );
            if !self.idle_verified {
                if idle.duration() > self.idle_seconds {
                    self.idle_verified = true;
                    Ok(Some(true))
                } else {
                    Ok(Some(false))
                }
            } else {
                if idle.duration() <= self.idle_seconds {
                    self.idle_verified = false;
                }
                Ok(Some(false))
            }
        } else {
            // in case of error, consider the condition NOT verified, but
            // with no side effects on internal status
            Ok(Some(false))
        }
    }
}

// end.
