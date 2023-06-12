//! Define an *execution-bucket* based condition
//!
//! This type of condition is verified when some event schedules the condition
//! for verification at the next scheduler tick. After a positive check (and
//! consecutive task execution) the condition is descheduled, until possibily
//! rescheduled by an external event.
//!
//! This is achieved by creating an *execution bucket*, containing the names of
//! conditions that have to be run at the next tick. At each tick the bucket is
//! checked, and all conditions (of this kind) are verified. Therefore, after
//! each tick, the execution bucket will be empty again.


use std::sync::Arc;
use std::sync::Mutex;
use std::time::Instant;
use std::collections::HashSet;

use cfgmap::CfgMap;

use super::base::Condition;
use crate::task::registry::TaskRegistry;
use crate::common::logging::{log, LogType};
use crate::constants::*;



/// Execution Bucket
/// 
/// Contains the names of the conditions that have to be executed at the next
/// tick. A name can be present only once, so multiple insertions are
/// automatically rejected (or _debounced_).
pub struct ExecutionBucket {
    execution_list: Arc<Mutex<HashSet<String>>>
}

#[allow(dead_code)]
impl ExecutionBucket {

    /// Create a new empty condition `ExecutionBucket`
    pub fn new() -> Self {
        ExecutionBucket {
            execution_list: Arc::new(Mutex::new(HashSet::new()))
        }
    }

    /// Return `true` if the condition name is in the bucket
    pub fn has_condition(&self, name: &str) -> bool {
        self.execution_list.clone().lock()
            .unwrap()
            .contains(&String::from(name))
    }

    /// Try to insert the condition in the bucket, return `false` if the name
    /// is already present, in which case the condition is not inserted
    pub fn insert_condition(&self, name: &str) -> bool {
        if !self.has_condition(name) {
            self.execution_list.clone().lock().unwrap().insert(String::from(name))
        } else {
            false
        }
    }

    /// Remove a condition if present and return `true`, `false` if not present
    pub fn remove_condition(&self, name: &str) -> bool {
        if self.has_condition(name) {
            self.execution_list.clone().lock().unwrap().remove(&String::from(name))
        } else {
            false
        }
    }
}



/// Execution Bucket Based Condition
///
/// This condition is verified when its name appears in the above-defined
/// execution bucket: upon verification the name is removed and the conditon
/// is descheduled.
pub struct BucketCondition {
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
    declared_type: String,

    // internal values
    execution_bucket: Option<&'static ExecutionBucket>,
}



#[allow(dead_code)]
impl BucketCondition {

    /// Create a new bucket/event condition with the provided name
    pub fn new(
        name: &str,
    ) -> Self {
        log(LogType::Debug, "BUCKET_CONDITION new",
            &format!("[INIT/MSG] CONDITION {name}: creating a new bucket based condition"));
        BucketCondition {
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
            declared_type: String::from("bucket"),

            // specific members initialization
            execution_bucket: None,
        }
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

    /// Set the execution bucket, which has to be defined at application level
    pub fn set_execution_bucket(&mut self, bucket: &'static ExecutionBucket) -> std::io::Result<bool> {
        self.execution_bucket = Some(bucket);
        Ok(true)
    }

    /// Load a `BucketCondition` from a [`CfgMap`](https://docs.rs/cfgmap/latest/)
    ///
    /// The `BucketCondition` is initialized according to the values provided
    /// in the `CfgMap` argument. If the `CfgMap` format does not comply with
    /// the requirements of a `BucketCondition` an error is raised.
    ///
    /// The TOML configuration file format is the following
    ///
    /// ```toml
    /// # definition (mandatory)
    /// [[condition]]
    /// name = "BucketConditionName"
    /// type = "bucket"         # "bucket" or "event" are the allowed values
    ///
    /// # optional parameters (if omitted, defaults are used)
    /// recurring = false
    /// execute_sequence = true
    /// break_on_failure = false
    /// break_on_success = false
    /// suspended = false
    /// tasks = [ "Task1", "Task2", ... ]
    /// ```
    ///
    /// Any incorrect value will cause an error. The value of the `type` entry
    /// *must* be set to `"bucket"` mandatorily for this type of `Condition`.
    pub fn load_cfgmap(cfgmap: &CfgMap, task_registry: &'static TaskRegistry) -> std::io::Result<BucketCondition> {

        fn _invalid_cfg(key: &str, value: &str, message: &str) -> std::io::Result<BucketCondition> {
            Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("invalid condition configuration: ({key}={value}) {message}"),
            ))
        }

        let check = vec!(
            "type",
            "name",
            "tasks",
            "recurring",
            "execute_sequence",
            "break_on_failure",
            "break_on_success",
            "suspended",
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
            if cond_type != "bucket" && cond_type != "event" {
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

        // initialize the structure
        // NOTE: the value of "event" for the condition type, which is
        //       completely functionally equivalent to "bucket", can only
        //       be set from the configuration file; programmatically built
        //       conditions of this type will only report "bucket" as their
        //       type, and "event" is only left for configuration readability
        let mut new_condition = BucketCondition::new(
            &name,
        );
        new_condition.task_registry = Some(&task_registry);
        new_condition.declared_type = String::from(cond_type);

        // by default make condition active if loaded from configuration: if
        // the configuration changes this state the condition will not start
        new_condition.suspended = false;

        // common optional parameter initialization
        let cur_key = "tasks";
        if let Some(item) = cfgmap.get(cur_key) {
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
                        &s,
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
        // (none here)

        // start the condition if the configuration did not suspend it
        if !new_condition.suspended {
            new_condition.start()?;
        }

        Ok(new_condition)
    }

}



impl Condition for BucketCondition {

    fn set_id(&mut self, id: i64) { self.cond_id = id; }
    fn get_name(&self) -> String { self.cond_name.clone() }
    fn get_id(&self) -> i64 { self.cond_id }
    fn get_type(&self) -> &str { self.declared_type.as_str() }


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
    /// This function actually performs the test by verifying whether or not
    /// its name is present in the common execution bucket: if present the
    /// name is removed to avoid subsequents executions (unless externally
    /// rescheduled), and the verification is successful
    fn _check_condition(&mut self) -> Result<Option<bool>, std::io::Error> {

        // last_tested has already been set by trait to Instant::now()
        self.log(
            LogType::Debug,
            &format!("[START/MSG] checking for presence in execution bucket")
        );
        if let Some(bucket) = self.execution_bucket {
            let name = self.get_name();
            if bucket.has_condition(&name) {
                bucket.remove_condition(&name);
                self.log(
                    LogType::Debug,
                    &format!("[END/OK] condition {name} verified and removed from bucket")
                );
                Ok(Some(true))
            } else {
                Ok(Some(false))
            }
        } else {
            panic!(
                "BUCKET_CONDITION condition {} used with undefined execution bucket",
                self.cond_name,
            );
        }
    }
}



// end.
