//! Define event based on file/directory changes (aka _notify_)
//!
//! The user states to watch certain files or directories, and the OS emits
//! a notification everytime that a change occurs in the watched items. This
//! event puts an associated condition into the execution bucket each time
//! it happens.

use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::PathBuf;
use std::time::Duration;

use async_trait::async_trait;
use futures::{
    SinkExt, StreamExt,
    channel::mpsc::{Receiver, channel},
};

use cfgmap::CfgMap;

use notify::{self, Watcher};

use super::base::Event;
use crate::common::logging::{LogType, log};
use crate::common::wres::{Error, Result};
use crate::condition::bucket_cond::ExecutionBucket;
use crate::condition::registry::ConditionRegistry;
use crate::{cfg_mandatory, constants::*};

use crate::cfghelp::*;

// default seconds to wayt between active polls: generally ignored
const DEFAULT_FSCHANGE_POLL_SECONDS: u64 = 2;

/// Filesystem Change Based Event
///
/// Implements an event based upon filesystem change notification: it uses the
/// cross-platform [`notify`](https://crates.io/crates/notify) crate to achieve
/// this. Reactions are allowed only for changing events, that is: pure access
/// will not fire any condition.
#[allow(dead_code)]
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
    event_rx: Option<Receiver<notify::Result<notify::Event>>>,
    event_watcher: Option<notify::RecommendedWatcher>,
}

// implement the hash protocol
impl Hash for FilesystemChangeEvent {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // common part
        self.event_name.hash(state);
        if let Some(s) = &self.condition_name {
            s.hash(state);
        }

        // specific part
        if let Some(x) = &self.watched_locations {
            let mut sorted = x.clone();
            sorted.sort();
            sorted.hash(state);
        } else {
            0.hash(state);
        }
        self.poll_seconds.hash(state);
        self.recursive.hash(state);
    }
}

// implement cloning
impl Clone for FilesystemChangeEvent {
    fn clone(&self) -> Self {
        FilesystemChangeEvent {
            // reset ID
            event_id: 0,

            // parameters
            event_name: self.event_name.clone(),
            condition_name: self.condition_name.clone(),

            // internal values
            condition_registry: None,
            condition_bucket: None,

            // specific members initialization
            // parameters
            watched_locations: self.watched_locations.clone(),
            poll_seconds: self.poll_seconds,
            recursive: self.recursive,

            // internal values
            event_rx: None,
            event_watcher: None,
        }
    }
}

#[allow(dead_code)]
impl FilesystemChangeEvent {
    pub fn new(name: &str) -> Self {
        log(
            LogType::Debug,
            LOG_EMITTER_EVENT_FSCHANGE,
            LOG_ACTION_NEW,
            Some((name, 0)),
            LOG_WHEN_INIT,
            LOG_STATUS_MSG,
            &format!("EVENT {name}: creating a new filesystem change based event"),
        );
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
            event_rx: None,
            event_watcher: None,
        }
    }

    /// Add a location to be watched.
    pub fn watch_location(&mut self, location: &str) -> Result<bool> {
        let p = PathBuf::from(location);
        if p.exists() {
            self.log(
                LogType::Debug,
                LOG_WHEN_START,
                LOG_STATUS_OK,
                &format!(
                    "found valid item to watch: `{}`",
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
                LOG_WHEN_END,
                LOG_STATUS_FAIL,
                &format!(
                    "refusing non-existent item: `{}`",
                    p.as_os_str().to_string_lossy(),
                ),
            );
            return Ok(false);
        }

        Ok(true)
    }

    /// State whether watching is recursive (on directories).
    pub fn set_recursive(&mut self, yes: bool) {
        self.recursive = yes;
    }
    pub fn recursive(&self) -> bool {
        self.recursive
    }

    /// Load a `FilesystemChangeEvent` from a [`CfgMap`](https://docs.rs/cfgmap/latest/)
    ///
    /// The `FilesystemChangeEvent` is initialized according to the values
    /// provided in the `CfgMap` argument. If the `CfgMap` format does not
    /// comply with the requirements of a `FilesystemChangeEvent` an error is
    /// raised.
    pub fn load_cfgmap(
        cfgmap: &CfgMap,
        cond_registry: &'static ConditionRegistry,
        bucket: &'static ExecutionBucket,
    ) -> Result<FilesystemChangeEvent> {
        let check = vec!["type", "name", "tags", "condition", "watch", "recursive"];
        cfg_check_keys(cfgmap, &check)?;

        // common mandatory parameter retrieval

        // type and name are both mandatory but type is only checked
        cfg_mandatory!(cfg_string_check_exact(cfgmap, "type", "fschange"))?;
        let name = cfg_mandatory!(cfg_string_check_regex(cfgmap, "name", &RE_EVENT_NAME))?.unwrap();

        // initialize the structure
        // NOTE: the value of "event" for the condition type, which is
        //       completely functionally equivalent to "bucket", can only
        //       be set from the configuration file; programmatically built
        //       conditions of this type will only report "bucket" as their
        //       type, and "event" is only left for configuration readability
        let mut new_event = FilesystemChangeEvent::new(&name);
        new_event.condition_registry = Some(cond_registry);
        new_event.condition_bucket = Some(bucket);

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

        let cur_key = "condition";
        if let Some(v) = cfg_string_check_regex(cfgmap, "condition", &RE_COND_NAME)? {
            if !new_event.condition_registry.unwrap().has_condition(&v) {
                return Err(cfg_err_invalid_config(
                    cur_key,
                    &v,
                    ERR_INVALID_EVENT_CONDITION,
                ));
            }
            new_event.assign_condition(&v)?;
        }

        // specific optional parameter initialization
        if let Some(v) = cfg_vec_string(cfgmap, "watch")? {
            for s in v {
                // let's just log the errors and avoid refusing invalid entries
                // if !new_event.watch_location(&s)? {
                //     return Err(cfg_err_invalid_config(
                //         cur_key,
                //         &s,
                //         ERR_INVALID_VALUE_FOR_ENTRY,
                //     ));
                // }
                let _ = new_event.watch_location(&s)?;
            }
        }

        if let Some(v) = cfg_bool(cfgmap, "recursive")? {
            new_event.recursive = v;
        }
        if let Some(v) = cfg_int_check_above_eq(cfgmap, "poll_seconds", 0)? {
            new_event.poll_seconds = v as u64;
        }

        Ok(new_event)
    }

    /// Check a configuration map and return item name if Ok
    ///
    /// The check is performed exactly in the same way and in the same order
    /// as in `load_cfgmap`, the only difference is that no actual item is
    /// created and that a name is returned, which is the name of the item that
    /// _would_ be created via the equivalent call to `load_cfgmap`
    pub fn check_cfgmap(cfgmap: &CfgMap, available_conditions: &Vec<&str>) -> Result<String> {
        let check = vec!["type", "name", "tags", "condition", "watch", "recursive"];
        cfg_check_keys(cfgmap, &check)?;

        // common mandatory parameter retrieval

        // type and name are both mandatory: type is checked and name is kept
        cfg_mandatory!(cfg_string_check_exact(cfgmap, "type", "fschange"))?;
        let name = cfg_mandatory!(cfg_string_check_regex(cfgmap, "name", &RE_EVENT_NAME))?.unwrap();

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

        // assigned condition is checked against the provided array
        let cur_key = "condition";
        if let Some(v) = cfg_string_check_regex(cfgmap, "condition", &RE_COND_NAME)? {
            if !available_conditions.contains(&v.as_str()) {
                return Err(cfg_err_invalid_config(
                    cur_key,
                    &v,
                    ERR_INVALID_EVENT_CONDITION,
                ));
            }
        }

        // specific optional parameter check
        cfg_vec_string(cfgmap, "watch")?; // see above: we do not check for correctness
        cfg_bool(cfgmap, "recursive")?;
        cfg_int_check_above_eq(cfgmap, "poll_seconds", 0)?;

        Ok(name)
    }
}

#[async_trait(?Send)]
impl Event for FilesystemChangeEvent {
    fn set_id(&mut self, id: i64) {
        self.event_id = id;
    }
    fn get_name(&self) -> String {
        self.event_name.clone()
    }
    fn get_id(&self) -> i64 {
        self.event_id
    }

    /// Return a hash of this item for comparison
    fn _hash(&self) -> u64 {
        let mut s = DefaultHasher::new();
        self.hash(&mut s);
        s.finish()
    }

    fn get_condition(&self) -> Option<String> {
        self.condition_name.clone()
    }

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

    // this function is a wrapper for the actual asynchronous event receiver
    async fn event_triggered(&mut self) -> Result<Option<String>> {
        let name = self.get_name();
        assert!(
            self.event_rx.is_some(),
            "uninitialized event notification channel for FilesystemChangeEvent {name}",
        );
        assert!(
            self.event_watcher.is_some(),
            "uninitialized event notifier for FilesystemChangeEvent {name}",
        );
        let event_receiver = self.event_rx.as_mut().unwrap();

        // this already returns the selected value
        while let Some(evt) = event_receiver.next().await {
            // ignore access events
            let evt = evt?;
            if !evt.kind.is_access() {
                let evt_s = if evt.kind.is_create() {
                    "CREATE"
                } else if evt.kind.is_modify() {
                    "MODIFY"
                } else if evt.kind.is_remove() {
                    "REMOVE"
                } else if evt.kind.is_other() {
                    "OTHER"
                } else {
                    "UNKNOWN"
                };
                self.log(
                    LogType::Debug,
                    LOG_WHEN_PROC,
                    LOG_STATUS_OK,
                    &format!("event notification caught: {evt_s}"),
                );
                match self.fire_condition() {
                    Ok(res) => {
                        if res {
                            self.log(
                                LogType::Debug,
                                LOG_WHEN_PROC,
                                LOG_STATUS_OK,
                                "condition fired successfully",
                            );
                        } else {
                            self.log(
                                LogType::Trace,
                                LOG_WHEN_PROC,
                                LOG_STATUS_MSG,
                                "condition already fired: further schedule skipped",
                            );
                        }
                    }
                    Err(e) => {
                        self.log(
                            LogType::Warn,
                            LOG_WHEN_PROC,
                            LOG_STATUS_FAIL,
                            &format!("error firing condition: {e}"),
                        );
                        return Err(Error::from(e));
                    }
                }
                break;
            } else {
                // an access only notification is a non triggered event
                self.log(
                    LogType::Debug,
                    LOG_WHEN_PROC,
                    LOG_STATUS_OK,
                    &format!("access-only event notification caught"),
                );
                return Ok(None);
            }
        }

        Ok(Some(name))
    }

    fn prepare_listener(&mut self) -> Result<bool> {
        assert!(
            self.event_rx.is_none(),
            "event listening channel for FilesystemChangeEvent {} is already initialized",
            self.get_name(),
        );

        // bail out if no location to watch has been provided
        if self.watched_locations.is_none() {
            self.log(
                LogType::Error,
                LOG_WHEN_START,
                LOG_STATUS_FAIL,
                "watch locations not specified",
            );
            return Ok(false);
        }

        // see: https://github.com/notify-rs/notify/blob/main/examples/async_monitor.rs
        fn _build_watcher(
            notify_cfg: notify::Config,
        ) -> notify::Result<(
            notify::RecommendedWatcher,
            Receiver<notify::Result<notify::Event>>,
        )> {
            let (mut tx, rx) = channel(EVENT_CHANNEL_SIZE);

            let watcher = notify::RecommendedWatcher::new(
                move |res| {
                    futures::executor::block_on(async {
                        tx.send(res).await.unwrap();
                    })
                },
                notify_cfg,
            )?;
            Ok((watcher, rx))
        }

        // build a configuration
        let notify_cfg =
            notify::Config::default().with_poll_interval(Duration::from_secs(self.poll_seconds));

        // choose directory recursive mode
        let recmode = if self.recursive {
            notify::RecursiveMode::Recursive
        } else {
            notify::RecursiveMode::NonRecursive
        };

        // and now build the watcher and the receiving channel to be saved
        let (mut watcher, event_rx) = _build_watcher(notify_cfg)?;

        let wl = self.watched_locations.clone().unwrap();
        for p in wl {
            match watcher.watch(&p, recmode) {
                Ok(_) => {
                    self.log(
                        LogType::Debug,
                        LOG_WHEN_START,
                        LOG_STATUS_OK,
                        &format!(
                            "successfully added `{}` to watched paths",
                            p.as_os_str().to_string_lossy(),
                        ),
                    );
                }
                Err(e) => {
                    self.log(
                        LogType::Warn,
                        LOG_WHEN_START,
                        LOG_STATUS_FAIL,
                        &format!(
                            "could not add `{}` to watched paths: {e}",
                            p.as_os_str().to_string_lossy(),
                        ),
                    );
                }
            }
        }

        // now that everything is set up, assign correct channels to the
        // event and preserve the watcher from destruction
        self.event_rx = Some(event_rx);
        self.event_watcher = Some(watcher);

        Ok(true)
    }
}

// end.
