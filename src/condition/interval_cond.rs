//! Define a time interval based condition
//!
//! This type of condition is verified at least once if the interval specified
//! on construction has passed since the condition activation, and if set to be
//! recurring it is verified every time that the same amount of time has passed
//! since the last positive verification.


use std::time::{Instant, Duration};

use cfgmap::CfgMap;

use super::base::Condition;
use crate::task::registry::TaskRegistry;
use crate::common::logging::{log, LogType};
use crate::constants::*;



/// Time Interval Based Condition
///
/// This condition is verified once enough time has passed since it has been
/// started, or since it last succeeded if it is a recurrent condition.
pub struct IntervalCondition {
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
    interval: Duration,

    // internal values
    checked: Instant,
}


#[allow(dead_code)]
impl IntervalCondition {

    /// Create a new time interval based condition with the given name and
    /// interval time duration
    pub fn new(
        name: &str,
        interval: &Duration,
    ) -> Self {
        log(
            LogType::Debug,
            LOG_EMITTER_CONDITION_INTERVAL,
            LOG_ACTION_NEW,
            Some((name, 0)),
            LOG_WHEN_INIT,
            LOG_STATUS_MSG,
            &format!("CONDITION {name}: creating a new interval based condition"),
        );
        let t = Instant::now();
        IntervalCondition {
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
            interval: interval.clone(),

            // specific members initialization
            checked: t,
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

    /// Load an `IntervalCondition` from a [`CfgMap`](https://docs.rs/cfgmap/latest/)
    ///
    /// The `IntervalCondition` is initialized according to the values provided
    /// in the `CfgMap` argument. If the `CfgMap` format does not comply with
    /// the requirements of a `IntervalCondition` an error is raised.
    ///
    /// The TOML configuration file format is the following
    ///
    /// ```toml
    /// # definition (mandatory)
    /// [[condition]]
    /// name = "IntervalConditionName"
    /// type = "interval"                           # mandatory value
    ///
    /// interval_seconds = 3600
    ///
    /// # optional parameters (if omitted, defaults are used)
    /// recurring = false
    /// execute_sequence = true
    /// break_on_failure = false
    /// break_on_success = false
    /// suspended = true
    /// tasks = [ "Task1", "Task2", ... ]
    /// ```
    ///
    /// Any incorrect value will cause an error. The value of the `type` entry
    /// *must* be set to `"interval"` mandatorily for this type of `Condition`.
    pub fn load_cfgmap(cfgmap: &CfgMap, task_registry: &'static TaskRegistry) -> std::io::Result<IntervalCondition> {

        fn _invalid_cfg(key: &str, value: &str, message: &str) -> std::io::Result<IntervalCondition> {
            Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("{ERR_INVALID_COND_CONFIG}: ({key}={value}) {message}"),
            ))
        }

        let check = [
            "type",
            "name",
            "tags",
            "interval_seconds",
            "tasks",
            "recurring",
            "execute_sequence",
            "break_on_failure",
            "break_on_success",
            "suspended",
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
            if cond_type != "interval" {
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

        // specific mandatory parameter retrieval
        let cur_key = "interval_seconds";
        let interval: Duration;
        if let Some(item) = cfgmap.get(cur_key) {
            if !item.is_int() {
                return _invalid_cfg(
                    cur_key,
                    STR_UNKNOWN_VALUE,
                    ERR_INVALID_PARAMETER);
            } else {
                let i = *item.as_int().unwrap();
                if i < 0 {
                    return _invalid_cfg(
                        cur_key,
                        &i.to_string(),
                        ERR_INVALID_PARAMETER);
                }
                interval = Duration::from_secs(i as u64);
            }
        } else {
            return _invalid_cfg(
                cur_key,
                STR_UNKNOWN_VALUE,
                ERR_MISSING_PARAMETER);
        }

        // initialize the structure
        let mut new_condition = IntervalCondition::new(
            &name,
            &interval,
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

        // specific optional parameter initialization
        // (none here)

        // start the condition if the configuration did not suspend it
        if !new_condition.suspended {
            new_condition.start()?;
        }

        Ok(new_condition)
    }

}



impl Condition for IntervalCondition {

    fn set_id(&mut self, id: i64) { self.cond_id = id; }
    fn get_name(&self) -> String { self.cond_name.clone() }
    fn get_id(&self) -> i64 { self.cond_id }
    fn get_type(&self) -> &str { "interval" }


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
    /// This function actually performs the test: if at least `self.interval`
    /// time has passed since last successful check (which may be the initial
    /// check only if not recurring), the outcome is successful.
    fn _check_condition(&mut self) -> Result<Option<bool>, std::io::Error> {
        self.log(
            LogType::Debug,
            LOG_WHEN_PROC,
            LOG_STATUS_MSG,
            "checking interval based condition",
        );
        // last_tested has already been set by trait to Instant::now()
        let t = self.last_tested.unwrap();
        if self.interval <= t - self.checked {
            self.checked = t;
            Ok(Some(true))
        } else {
            Ok(Some(false))
        }
    }

}


// end.
