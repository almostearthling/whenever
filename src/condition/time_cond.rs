//! Define a time based condition
//!
//! This type of condition is verified when the current time is equal (or has
//! just passed) one of the provided time specifications. Time specifications
//! include:
//!
//! * the time of day, specified as
//!     - hours
//!     - minutes
//!     - seconds
//! * the date, specified as
//!     - day
//!     - month
//!     - year
//! * the day of the week.
//!
//! All values should be provided. If minute and second are not provided, they
//! are both considered the beginning of the hour. All other values, if not
//! provided, are considered to be always verified.


use std::time::Instant;
use std::hash::{DefaultHasher, Hash, Hasher};

use chrono::prelude::*;
use cfgmap::CfgMap;

use super::base::Condition;
use crate::task::registry::TaskRegistry;
use crate::common::logging::{log, LogType};
use crate::constants::*;



// the constructor only allows to specify all values on invocation, so this
// structure remainss private
struct TimeSpecification {
    year: Option<i32>,          // e.g. 2023
    month: Option<u32>,         // January=1, ..., December=12
    day: Option<u32>,           // 0, ..., 31
    dow: Option<u32>,           // Sunday=1, ..., Saturday=7
    hour: Option<u32>,          // 0, ..., 23
    minute: Option<u32>,        // 0, ..., 59
    second: Option<u32>,        // 0, ..., 59
}


// here it's easier to implement the hash protocol for a timespec
impl Hash for TimeSpecification {
    fn hash<H: Hasher>(&self, state: &mut H) {
        if let Some(x) = self.year {
            x.hash(state);
        } else {
            0.hash(state);
        }
        if let Some(x) = self.month {
            x.hash(state);
        } else {
            0.hash(state);
        }
        if let Some(x) = self.day {
            x.hash(state);
        } else {
            0.hash(state);
        }
        if let Some(x) = self.dow {
            x.hash(state);
        } else {
            0.hash(state);
        }
        if let Some(x) = self.hour {
            x.hash(state);
        } else {
            0.hash(state);
        }
        if let Some(x) = self.minute {
            x.hash(state);
        } else {
            0.hash(state);
        }
        if let Some(x) = self.second {
            x.hash(state);
        } else {
            0.hash(state);
        }
    }
}


impl TimeSpecification {

    /// Create a new time specification with the provided optional values
    pub fn new(
        year: Option<i32>,
        month: Option<u32>,
        day: Option<u32>,
        dow: Option<u32>,
        hour: Option<u32>,
        minute: Option<u32>,
        second: Option<u32>,
    ) -> TimeSpecification {
        TimeSpecification { year, month, day, dow, hour, minute, second }
    }

    /// returns the resulting date and time using the fields of the `now`
    /// parameter for the missing values
    pub fn as_datetime(&self, now: DateTime<Local>) -> std::io::Result<DateTime<Local>> {
        let year;
        let month;
        let day;
        // let dow;
        let hour;
        let minute;
        let second;
        if let Some(v) = self.year { year = v; } else { year = now.year(); }
        if let Some(v) = self.month { month = v; } else { month = now.month(); }
        if let Some(v) = self.day { day = v; } else { day = now.day(); }
        // if let Some(v) = self.dow { dow = v; } else { dow = now.weekday().number_from_sunday(); }
        if let Some(v) = self.hour { hour = v; } else { hour = now.hour(); }
        // if let Some(v) = self.minute { minute = v; } else { minute = now.minute(); }
        // if let Some(v) = self.second { second = v; } else { second = now.second(); }
        if let Some(v) = self.minute { minute = v; } else { minute = 0; }
        if let Some(v) = self.second { second = v; } else { second = 0; }

        let dt = Local.with_ymd_and_hms(year, month, day, hour, minute, second);
        match dt {
            chrono::offset::LocalResult::Single(_) => { }
            _ => {
                return Err(
                    std::io::Error::new(std::io::ErrorKind::InvalidData,
                    ERR_INVALID_TIMESPEC,
                ));
            }
        }

        Ok(dt.unwrap())
    }

    pub fn as_str(&self) -> String {
        format!(
            "{}-{}-{}T{}:{}:{} [{}]",
            { if let Some(n) = self.year { format!("{:04}", n) } else { String::from("____") } },
            { if let Some(n) = self.month { format!("{:02}", n) } else { String::from("__") } },
            { if let Some(n) = self.day { format!("{:02}", n) } else { String::from("__") } },
            { if let Some(n) = self.hour { format!("{:02}", n) } else { String::from("__") } },
            { if let Some(n) = self.minute { format!("{:02}", n) } else { String::from("__") } },
            { if let Some(n) = self.second { format!("{:02}", n) } else { String::from("__") } },
            {
                if let Some(dow) = self.dow {
                    match dow {
                        1 => "sun",
                        2 => "mon",
                        3 => "tue",
                        4 => "wed",
                        5 => "thu",
                        6 => "fri",
                        7 => "sat",
                        _ => "???",
                    }
                } else { "___" }
            }
        )
    }
}



/// Time Interval Based Condition
///
/// This condition is verified once enough time has passed since it has been
/// started, or since it last succeeded if it is a recurrent condition.
pub struct TimeCondition {
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
    time_specifications: Vec<TimeSpecification>,
    tick_duration: i64,
}


// implement the hash protocol
impl Hash for TimeCondition {
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
        self.tick_duration.hash(state);

        // time specifications support the hash protocol
        self.time_specifications.hash(state);
    }
}


#[allow(dead_code)]
impl TimeCondition {

    /// Create a new time based condition with the given name
    pub fn new(
        name: &str,
    ) -> Self {
        log(
            LogType::Debug,
            "TIME_CONDITION",
            LOG_ACTION_NEW,
            Some((name, 0)),
            LOG_WHEN_INIT,
            LOG_STATUS_MSG,
            &format!("CONDITION {name}: creating a new time based condition"),
        );
        TimeCondition {
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
            time_specifications: Vec::new(),
            tick_duration: 0,
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

    /// Set tick duration after creation
    pub fn set_tick_duration(&mut self, seconds: u64) -> std::io::Result<bool> {
        if seconds < 1 || seconds > std::i64::MAX as u64 {
            Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("{ERR_INVALID_TICK_SECONDS}: {seconds}")
            ))
        } else {
            self.tick_duration = seconds as i64;
            Ok(true)
        }
    }

    pub fn add_time_specification(
        &mut self,
        year: Option<i32>,
        month: Option<u32>,
        day: Option<u32>,
        hour: Option<u32>,
        minute: Option<u32>,
        second: Option<u32>,
        dow: Option<u32>,
    ) -> std::io::Result<bool> {
        if let Some(n) = year {
            if n < 0 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("{ERR_INVALID_VALUE_FOR} `{}`: {n}", "year"))
                );
            }
        }
        if let Some(n) = month {
            if !(1..=12).contains(&n) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("{ERR_INVALID_VALUE_FOR} `{}`: {n}", "month"))
                );
            }
        }
        if let Some(n) = day {
            if !(1..=31).contains(&n) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("{ERR_INVALID_VALUE_FOR} `{}`: {n}", "day"))
                );
            }
        }
        if let Some(n) = hour {
            if n > 23 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("{ERR_INVALID_VALUE_FOR} `{}`: {n}", "hour"))
                );
            }
        }
        if let Some(n) = minute {
            if n > 59 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("{ERR_INVALID_VALUE_FOR} `{}`: {n}", "minute"))
                );
            }
        }
        if let Some(n) = second {
            if n > 59 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("{ERR_INVALID_VALUE_FOR} `{}`: {n}", "second"))
                );
            }
        }
        if let Some(n) = dow {
            if !(1..=7).contains(&n) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("{ERR_INVALID_VALUE_FOR} `{}`: {n}", "weekday"))
                );
            }
        }
        self.time_specifications.push(
            TimeSpecification::new(year, month, day, dow, hour, minute, second));

        Ok(true)
    }

    /// Load an `TimeCondition` from a [`CfgMap`](https://docs.rs/cfgmap/latest/)
    ///
    /// The `IntervalCondition` is initialized according to the values provided
    /// in the `CfgMap` argument. If the `CfgMap` format does not comply with
    /// the requirements of a `TimeCondition` an error is raised.
    ///
    /// The TOML configuration file format is the following
    ///
    /// ```toml
    /// # definition (mandatory)
    /// [[condition]]
    /// name = "TimeConditionName"
    /// type = "time"                               # mandatory value
    ///
    /// # optional parameters (if omitted, defaults are used)
    /// time_specifications = [
    ///     { hour = 17, minute = 30 },
    ///     { hour = 12, minute = 0, weekday = "wed" },
    ///     ]
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
    pub fn load_cfgmap(cfgmap: &CfgMap, task_registry: &'static TaskRegistry) -> std::io::Result<TimeCondition> {

        fn _invalid_cfg(key: &str, value: &str, message: &str) -> std::io::Result<TimeCondition> {
            Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("{ERR_INVALID_COND_CONFIG}: ({key}={value}) {message}"),
            ))
        }

        let check = [
            "type",
            "name",
            "tags",
            "time_specifications",
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
            if cond_type != "time" {
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
        // (none here)

        // initialize the structure
        let mut new_condition = TimeCondition::new(
            &name,
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
        let cur_key = "time_specifications";
        if let Some(item) = cfgmap.get(cur_key) {
            if !item.is_list() {
                return _invalid_cfg(
                    cur_key,
                    STR_UNKNOWN_VALUE,
                    ERR_INVALID_PARAMETER);
            } else {
                for map in item.as_list().unwrap() {
                    if !map.is_map() {
                        return _invalid_cfg(
                            cur_key,
                            STR_UNKNOWN_VALUE,
                            ERR_INVALID_TIMESPEC);
                    } else {
                        let map = map.as_map().unwrap();
                        let year;
                        let month;
                        let day;
                        let hour;
                        let minute;
                        let second;
                        let dow;
                        if let Some(v) = map.get("year") {
                            if !v.is_int() {
                                return _invalid_cfg(
                                    cur_key,
                                    STR_UNKNOWN_VALUE,
                                    ERR_INVALID_TIMESPEC);
                            } else {
                                year = Some(*v.as_int().unwrap() as i32);
                            }
                        } else {
                            year = None;
                        }
                        if let Some(v) = map.get("month") {
                            if !v.is_int() {
                                return _invalid_cfg(
                                    cur_key,
                                    STR_UNKNOWN_VALUE,
                                    ERR_INVALID_TIMESPEC);
                            } else {
                                month = Some(*v.as_int().unwrap() as u32);
                            }
                        } else {
                            month = None;
                        }
                        if let Some(v) = map.get("day") {
                            if !v.is_int() {
                                return _invalid_cfg(
                                    cur_key,
                                    STR_UNKNOWN_VALUE,
                                    ERR_INVALID_TIMESPEC);
                            } else {
                                day = Some(*v.as_int().unwrap() as u32);
                            }
                        } else {
                            day = None;
                        }
                        if let Some(v) = map.get("hour") {
                            if !v.is_int() {
                                return _invalid_cfg(
                                    cur_key,
                                    STR_UNKNOWN_VALUE,
                                    ERR_INVALID_TIMESPEC);
                            } else {
                                hour = Some(*v.as_int().unwrap() as u32);
                            }
                        } else {
                            hour = None;
                        }
                        if let Some(v) = map.get("minute") {
                            if !v.is_int() {
                                return _invalid_cfg(
                                    cur_key,
                                    STR_UNKNOWN_VALUE,
                                    ERR_INVALID_TIMESPEC);
                            } else {
                                minute = Some(*v.as_int().unwrap() as u32);
                            }
                        } else {
                            minute = None;
                        }
                        if let Some(v) = map.get("second") {
                            if !v.is_int() {
                                return _invalid_cfg(
                                    cur_key,
                                    STR_UNKNOWN_VALUE,
                                    ERR_INVALID_TIMESPEC);
                            } else {
                                second = Some(*v.as_int().unwrap() as u32);
                            }
                        } else {
                            second = None;
                        }
                        if let Some(v) = map.get("weekday") {
                            if !v.is_str() {
                                return _invalid_cfg(
                                    cur_key,
                                    STR_UNKNOWN_VALUE,
                                    ERR_INVALID_TIMESPEC);
                            } else {
                                let s = v.as_str().unwrap().to_ascii_lowercase();
                                dow = Some(match s.as_str() {
                                    "sun" => 1,
                                    "sunday" => 1,
                                    "mon" => 2,
                                    "monday" => 2,
                                    "tue" => 3,
                                    "tuesday" => 3,
                                    "wed" => 4,
                                    "wednesday" => 4,
                                    "thu" => 5,
                                    "thursday" => 5,
                                    "fri" => 6,
                                    "friday" => 6,
                                    "sat" => 7,
                                    "saturday" => 7,
                                    _ => {
                                        return _invalid_cfg(
                                            cur_key,
                                            STR_UNKNOWN_VALUE,
                                            ERR_INVALID_TIMESPEC);
                                    },
                                })
                            }
                        } else {
                            dow = None;
                        }
                        let _ = new_condition.add_time_specification(
                            year, month, day, hour, minute, second, dow)?;
                    }
                }
            }
        } else {
            return _invalid_cfg(
                cur_key,
                STR_UNKNOWN_VALUE,
                ERR_MISSING_PARAMETER);
        }

        // start the condition if the configuration did not suspend it
        if !new_condition.suspended {
            new_condition.start()?;
        }

        Ok(new_condition)
    }

}



impl Condition for TimeCondition {

    fn set_id(&mut self, id: i64) { self.cond_id = id; }
    fn get_name(&self) -> String { self.cond_name.clone() }
    fn get_id(&self) -> i64 { self.cond_id }
    fn get_type(&self) -> &str { "time" }

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
    /// This function actually performs the test: if at least `self.interval`
    /// time has passed since last successful check (which may be the initial
    /// check only if not recurring), the outcome is successful.
    fn _check_condition(&mut self) -> Result<Option<bool>, std::io::Error> {
        if self.tick_duration <= 0 {
            return Err(
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    ERR_INVALID_TICK_SECONDS,
                ));
        }

        let dt = Local::now();
        self.log(
            LogType::Debug,
            LOG_WHEN_START,
            LOG_STATUS_MSG,
            &format!(
                "checking time based condition (at: {})",
                dt.format("%Y-%m-%dT%H:%M:%S [%a]")),
        );

        for tspec in self.time_specifications.iter() {
            let test_tspec = tspec.as_datetime(dt)?;
            self.log(
                LogType::Debug,
                LOG_WHEN_PROC,
                LOG_STATUS_MSG,
                &format!(
                    "checking time specification ({}) against current time",
                    tspec.as_str()),
            );
            // check that the required time has passed for less time than the
            // tick duration (which must be exactly the same as the scheduler
            // tick), and if so also check the week day (if it has been set)
            let span = (test_tspec - dt).num_microseconds().unwrap();
            if span > 0 && span < self.tick_duration * 1_000_000 {
                if let Some(dow) = tspec.dow {
                    if dow == dt.weekday().number_from_sunday() {
                        return Ok(Some(true));
                    } else {
                        return Ok(Some(false));
                    }
                } else {
                    return Ok(Some(true));
                }
            }
        }
        Ok(Some(false))
    }

}


// end.
