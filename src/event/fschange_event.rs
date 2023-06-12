//! Define event based on file/directory changes (aka _notify_)
//!
//! The user states to watch certain files or directories, and the OS emits
//! a notification everytime that a change occurs in the watched items. This
//! event puts an associated condition into the execution bucket each time
//! it happens.


use std::path::PathBuf;
use std::time::Duration;

use notify::{self, Watcher};
use cfgmap::CfgMap;

use super::base::Event;
use crate::condition::registry::ConditionRegistry;
use crate::condition::bucket_cond::ExecutionBucket;
use crate::common::logging::{log, LogType};
use crate::constants::*;



// default seconds to wayt between active polls: generally ignored
const DEFAULT_FSCHANGE_POLL_SECONDS: u64 = 2;



/// Filesystem Change Based Event
///
/// Implements an event based upon filesystem change notification: it uses the
/// cross-platform [`notify`](https://crates.io/crates/notify) crate to achieve
/// this. Reactions are allowed only for changing events, that is: pure access
/// will not fire any condition.
pub struct FilesystemChangeEvent {
    // common members
    // parameters
    event_id: i64,
    event_name: String,
    condition_name: Option<String>,

    // internal values
    condition_registry: Option<&'static ConditionRegistry>,
    condition_bucket: Option<&'static ExecutionBucket>,

    // specific members
    // parameters
    watched_locations: Option<Vec<PathBuf>>,
    poll_seconds: u64,
    recursive: bool,

    // internal values
}



#[allow(dead_code)]
impl FilesystemChangeEvent {
    pub fn new(name: &str) -> Self {
        log(LogType::Debug, "EVENT_FSCHANGE new",
            &format!("[INIT/MSG] EVENT {name}: creating a new filesystem change based event"));
        FilesystemChangeEvent {
            // reset ID
            event_id: 0,

            // parameters
            event_name: String::from(name),
            condition_name: None,

            // internal values
            condition_registry: None,
            condition_bucket: None,

            // specific members initialization
            // parameters
            watched_locations: None,
            poll_seconds: DEFAULT_FSCHANGE_POLL_SECONDS,
            recursive: false,

            // internal values
        }
    }

    /// Add a location to be watched.
    pub fn watch_location(&mut self, location: &str) -> std::io::Result<bool> {
        let p = PathBuf::from(location);
        if p.exists() {
            self.log(
                LogType::Debug,
                &format!(
                    "[INIT/OK] found valid item to watch: `{}`",
                    p.as_os_str().to_string_lossy(),
                ),
            );
            if self.watched_locations.is_none() {
                self.watched_locations = Some(Vec::new());
            }
            let mut wl = self.watched_locations.take().unwrap();
            wl.push(p);
            self.watched_locations = Some(wl);
        } else {
            self.log(
                LogType::Warn,
                &format!(
                    "[INIT/FAIL] refusing non-existent item: `{}`",
                    p.as_os_str().to_string_lossy(),
                ),
            );
            return Ok(false);
        }

        Ok(true)
    }

    /// State whether watching is recursive (on directories).
    pub fn set_recursive(&mut self, yes: bool) { self.recursive = yes; }
    pub fn recursive(&self) -> bool { self.recursive }


    /// Load a `FilesystemChangeEvent` from a [`CfgMap`](https://docs.rs/cfgmap/latest/)
    ///
    /// The `FilesystemChangeEvent` is initialized according to the values
    /// provided in the `CfgMap` argument. If the `CfgMap` format does not
    /// comply with the requirements of a `FilesystemChangeEvent` an error is
    /// raised.
    ///
    /// The TOML configuration file format is the following
    ///
    /// ```toml
    /// # definition (mandatory)
    /// [[event]]
    /// name = "FilesystemChangeEventName"
    /// type = "fschange"
    /// condition = "AssignedConditionName"
    ///
    /// # optional parameters (if omitted, defaults are used)
    /// watch = [
    ///     "/path/to/resource1",
    ///     "/path/to/resource2",
    ///     ...
    ///     ]
    /// recursive = false       // only applied to directories
    /// poll_seconds = 2        // ignored on most platforms
    /// ```
    ///
    /// Note that the pointed-to resources must exist in the file system,
    /// otherwise initialization will fail (FIXME: is it correct to act like
    /// this? Maybe one would wait for creation of a non existing resource);
    /// to watch for something that still does not exist, the parent directory
    /// should be watched instead.
    ///
    /// Any incorrect value will cause an error. The value of the `type` entry
    /// *must* be set to `"fschange"` mandatorily for this type of `Event`.
    pub fn load_cfgmap(
        cfgmap: &CfgMap,
        cond_registry: &'static ConditionRegistry,
        bucket: &'static ExecutionBucket,
    ) -> std::io::Result<FilesystemChangeEvent> {

        fn _invalid_cfg(key: &str, value: &str, message: &str) -> std::io::Result<FilesystemChangeEvent> {
            Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("invalid event configuration: ({key}={value}) {message}"),
            ))
        }

        let check = vec!(
            "type",
            "name",
            "condition",
            "watch",
            "recursive",
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
                    ERR_INVALID_EVENT_TYPE);
            }
            cond_type = item.as_str().unwrap().to_owned();
            if cond_type != "fschange" {
                return _invalid_cfg(&cur_key,
                    &cond_type,
                    ERR_INVALID_EVENT_TYPE);
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
                    ERR_INVALID_EVENT_NAME);
            }
            name = item.as_str().unwrap().to_owned();
            if !RE_EVENT_NAME.is_match(&name) {
                return _invalid_cfg(&cur_key,
                    &name,
                    ERR_INVALID_EVENT_NAME);
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
        let mut new_event = FilesystemChangeEvent::new(
            &name,
        );
        new_event.condition_registry = Some(&cond_registry);
        new_event.condition_bucket = Some(&bucket);

        // common optional parameter initialization
        let cur_key = "condition";
        let condition;
        if let Some(item) = cfgmap.get(&cur_key) {
            if !item.is_str() {
                return _invalid_cfg(
                    &cur_key,
                    STR_UNKNOWN_VALUE,
                    ERR_INVALID_EVENT_NAME);
            }
            condition = item.as_str().unwrap().to_owned();
            if !RE_COND_NAME.is_match(&condition) {
                return _invalid_cfg(&cur_key,
                    &condition,
                    ERR_INVALID_COND_NAME);
            }
        } else {
            return _invalid_cfg(
                &cur_key,
                STR_UNKNOWN_VALUE,
                ERR_MISSING_PARAMETER);
        }
        if !new_event.condition_registry.unwrap().has_condition(&condition) {
            return _invalid_cfg(
                &cur_key,
                &condition,
                ERR_INVALID_EVENT_CONDITION);
        }
        new_event.assign_condition(&condition)?;

        // specific optional parameter initialization
        let cur_key = "watch";
        if let Some(item) = cfgmap.get(cur_key) {
            if !item.is_list() {
                return _invalid_cfg(
                    &cur_key,
                    STR_UNKNOWN_VALUE,
                    ERR_INVALID_PARAMETER);
            }
            for a in item.as_list().unwrap() {
                let s = String::from(a.as_str().unwrap_or(&String::new()));
                // let's just log the errors and avoid refusing invalid entries
                // if !new_event.watch_location(&s)? {
                //     return _invalid_cfg(
                //         &cur_key,
                //         &s,
                //         ERR_INVALID_VALUE_FOR_ENTRY);
                // }
                let _ = new_event.watch_location(&s)?;
            }
        }

        let cur_key = "recursive";
        if let Some(item) = cfgmap.get(cur_key) {
            if !item.is_bool() {
                return _invalid_cfg(
                    &cur_key,
                    STR_UNKNOWN_VALUE,
                    ERR_INVALID_PARAMETER);
            } else {
                new_event.recursive = *item.as_bool().unwrap();
            }
        }

        let cur_key = "poll_secoonds";
        if let Some(item) = cfgmap.get(cur_key) {
            if !item.is_int() {
                return _invalid_cfg(
                    &cur_key,
                    STR_UNKNOWN_VALUE,
                    ERR_INVALID_PARAMETER);
            } else {
                new_event.poll_seconds = *item.as_int().unwrap() as u64;
            }
        }

        Ok(new_event)
    }

}


impl Event for FilesystemChangeEvent {

    fn set_id(&mut self, id: i64) { self.event_id = id; }
    fn get_name(&self) -> String { self.event_name.clone() }
    fn get_id(&self) -> i64 { self.event_id }

    fn requires_thread(&self) -> bool { true }  // maybe false, let's see

    fn get_condition(&self) -> Option<String> { self.condition_name.clone() }

    fn set_condition_registry(&mut self, reg: &'static ConditionRegistry) {
        self.condition_registry = Some(reg);
    }

    fn condition_registry(&self) -> Option<&'static ConditionRegistry> {
        self.condition_registry
    }

    fn set_condition_bucket(&mut self, bucket: &'static ExecutionBucket) {
        self.condition_bucket = Some(bucket);
    }

    fn condition_bucket(&self) -> Option<&'static ExecutionBucket> {
        self.condition_bucket
    }

    fn _assign_condition(&mut self, cond_name: &str) {
        // correctness has already been checked by the caller
        self.condition_name = Some(String::from(cond_name));
    }


    fn _start_service(&self) -> std::io::Result<bool> {

        if self.watched_locations.is_none() {
            self.log(
                LogType::Error,
                &format!(
                    "[START/FAIL] watch locations not specified",
                ),
            );
            return Ok(false);
        }

        // clone the location as a pathbuf (will not panic: checked above)
        let rec = if self.recursive {
            notify::RecursiveMode::Recursive
        } else {
            notify::RecursiveMode::NonRecursive
        };

        // for the watching loop technique, see the official notify example
        // at: examples/watcher_kind.rs
        let (tx, rx) = std::sync::mpsc::channel();
        let notify_cfg = notify::Config::default()
            .with_poll_interval(Duration::from_secs(self.poll_seconds));

        let mut watcher = Box::new(
            notify::RecommendedWatcher::new(tx, notify_cfg).unwrap());

        // if the watcher could be set to watch filesystem events, then an
        // endless cycle to catch these events is started: in this case all
        // possible errors are treated as warnings from a logging point of
        // view; otherwise exit with Ok(false), which indicates an error
        if let Some(wl) = self.watched_locations.clone() {
            for p in wl {
                match watcher.watch(&p, rec) {
                    Ok(_) => {
                        self.log(
                            LogType::Debug,
                            &format!(
                                "[START/OK] successfully added `{}` to watched paths",
                                p.as_os_str().to_string_lossy(),
                            ),
                        );
                    }
                    Err(e) => {
                        self.log(
                            LogType::Warn,
                            &format!(
                                "[START/FAIL] could not add `{}` to watched paths: {}",
                                p.as_os_str().to_string_lossy(),
                                e.to_string(),
                            ),
                        );
                    }
                }
            }
        } else {
            self.log(
                LogType::Error,
                &format!("[START/FAIL] no paths to watch have been specified"),
            );
            return Ok(false);
        }

        for r_evt in rx {
            match r_evt {
                Ok(evt) => {
                    let evt_s = {
                        if evt.kind.is_access() { "ACCESS" } else
                        if evt.kind.is_create() { "CREATE" } else
                        if evt.kind.is_modify() { "MODIFY" } else
                        if evt.kind.is_remove() { "REMOVE" } else
                        if evt.kind.is_other() { "OTHER" } else { "UNKNOWN" }
                    };
                    // ignore access events
                    if !evt.kind.is_access() {
                        self.log(
                            LogType::Info,
                            &format!("[PROC/OK] event notification caught: {evt_s}"),
                        );
                        match self.fire_condition() {
                            Ok(_) => {
                                self.log(
                                    LogType::Debug,
                                    &format!("[PROC/OK] condition fired successfully"),
                                );
                            }
                            Err(e) => {
                                self.log(
                                    LogType::Warn,
                                    &format!("[PROC/FAIL] error firing condition: {}",
                                        e.to_string()),
                                );
                            }
                        }
                    } else {
                        self.log(
                            LogType::Debug,
                            &format!("[PROC/OK] non-change event notification caught: {evt_s}"),
                        );
                    }
                }
                Err(e) => {
                    self.log(
                        LogType::Warn,
                        &format!(
                            "[PROC/FAIL] error in event notification: {}",
                            e.to_string(),
                        ),
                    );
                }
            };
        }

        Ok(true)
    }

}


// end.
