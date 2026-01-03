//! Define event based on file/directory changes (aka _notify_)
//!
//! The user states to watch certain files or directories, and the OS emits
//! a notification everytime that a change occurs in the watched items. This
//! event puts an associated condition into the execution bucket each time
//! it happens.

use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::PathBuf;
use std::sync::{RwLock, mpsc};
use std::thread;
use std::time::Duration;

use async_std::task;
use async_trait::async_trait;

use futures::{
    SinkExt, StreamExt,
    channel::mpsc::{Receiver, Sender, channel},
};

use cfgmap::CfgMap;
use notify::{self, Watcher};

use super::base::Event;

use crate::common::logging::{LogType, log};
use crate::common::wres::{Error, Kind, Result};
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
    thread_running: RwLock<bool>,
    quit_tx: Option<mpsc::Sender<()>>,
    quit_rx: Option<Receiver<()>>,
    event_rx: Option<Receiver<notify::Result<notify::Event>>>,
    event_watcher: Option<notify::ReadDirectoryChangesWatcher>,
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
            thread_running: RwLock::new(false),
            quit_tx: None,
            quit_rx: None,
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
            thread_running: RwLock::new(false),
            quit_tx: None,
            quit_rx: None,
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
                    p.as_os_str().to_string_lossy()
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
                    p.as_os_str().to_string_lossy()
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

    fn requires_thread(&self) -> bool {
        true
    } // maybe false, let's see

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

    fn assign_quit_sender(&mut self, sr: mpsc::Sender<()>) {
        assert!(
            self.get_id() != 0,
            "event {} not registered",
            self.get_name(),
        );
        self.quit_tx = Some(sr);
    }

    fn run_service(&self, qrx: Option<mpsc::Receiver<()>>) -> Result<bool> {
        assert!(
            qrx.is_some(),
            "quit signal channel receiver must be provided",
        );
        assert!(
            self.quit_tx.is_some(),
            "quit signal channel transmitter not initialized",
        );

        // unified event type that will be sent over an async channel by
        // either a `quit` command or the watcher: the `Target` option
        // contains the event generated by the watcher
        enum TargetOrQuitEvent {
            Target(notify::Result<notify::Event>),
            Quit,
            QuitError,
        }

        // see: https://github.com/notify-rs/notify/blob/main/examples/async_monitor.rs
        // with the difference that this wraps the result in a `TargetOrQuitEvent`
        // and that the transmitting part of the channel must be provided by
        // the caller, therefore it only returns the watcher
        fn _build_watcher(
            cfg: notify::Config,
            mut atx: Sender<TargetOrQuitEvent>,
        ) -> notify::Result<notify::RecommendedWatcher> {
            let watcher = notify::RecommendedWatcher::new(
                move |res| {
                    futures::executor::block_on(async {
                        atx.send(TargetOrQuitEvent::Target(res)).await.unwrap();
                    })
                },
                cfg,
            )?;
            Ok(watcher)
        }

        if self.watched_locations.is_none() {
            self.log(
                LogType::Error,
                LOG_WHEN_START,
                LOG_STATUS_FAIL,
                "watch locations not specified",
            );
            return Ok(false);
        }

        let rec = if self.recursive {
            notify::RecursiveMode::Recursive
        } else {
            notify::RecursiveMode::NonRecursive
        };

        // for the watching loop technique, see the official notify example
        // at: examples/watcher_kind.rs

        // build an async communication channel: since two threads insist on
        // it, a suitable capacity is needed
        let (async_tx, mut async_rx) = channel(EVENT_QUIT_CHANNEL_SIZE);

        // build a configuration
        let notify_cfg =
            notify::Config::default().with_poll_interval(Duration::from_secs(self.poll_seconds));

        // and now build the watcher, passing a clone of the transmitting end
        // of the channel to the constructor
        let mut watcher = _build_watcher(notify_cfg, async_tx.clone())?;

        // add locations to the watcher
        if let Some(wl) = self.watched_locations.clone() {
            for p in wl {
                match watcher.watch(&p, rec) {
                    Ok(_) => {
                        self.log(
                            LogType::Debug,
                            LOG_WHEN_START,
                            LOG_STATUS_OK,
                            &format!(
                                "successfully added `{}` to watched paths",
                                p.as_os_str().to_string_lossy()
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
                                p.as_os_str().to_string_lossy()
                            ),
                        );
                    }
                }
            }
        } else {
            self.log(
                LogType::Error,
                LOG_WHEN_START,
                LOG_STATUS_FAIL,
                "could not acquire list of paths to watch",
            );
            return Ok(false);
        }

        // now it is time to set the internal `running` flag, before the
        // thread that waits for the quit signal is launched
        let mut running = self.thread_running.write().unwrap();
        *running = true;
        drop(running);

        // spawn a thread that only listens to a possible request to quit:
        // this thread should be lightweight enough, as it just waits all
        // the time; it is also useless to join to because it dies as soon
        // as it catches a signal
        let mut async_tx_clone = async_tx.clone();
        let _quit_handle = thread::spawn(move || {
            match qrx.unwrap().recv() {
                Ok(_) => {
                    // send a quit message over the async channel
                    task::block_on({
                        async move {
                            async_tx_clone.send(TargetOrQuitEvent::Quit).await.unwrap();
                        }
                    });
                }
                _ => {
                    // in case of error, send just the error option of the enum
                    task::block_on({
                        async move {
                            async_tx_clone
                                .send(TargetOrQuitEvent::QuitError)
                                .await
                                .unwrap();
                        }
                    });
                }
            };
        });

        // if the watcher could be set to watch filesystem events, then an
        // endless cycle to catch these events is started: in this case all
        // possible errors are treated as warnings from a logging point of
        // view; otherwise exit with Ok(false), which indicates an error;
        // this should be running in the local pool
        futures::executor::block_on(async move {
            while let Some(toq) = async_rx.next().await {
                match toq {
                    TargetOrQuitEvent::Target(r_evt) => {
                        match r_evt {
                            Ok(evt) => {
                                let evt_s = {
                                    if evt.kind.is_access() {
                                        "ACCESS"
                                    } else if evt.kind.is_create() {
                                        "CREATE"
                                    } else if evt.kind.is_modify() {
                                        "MODIFY"
                                    } else if evt.kind.is_remove() {
                                        "REMOVE"
                                    } else if evt.kind.is_other() {
                                        "OTHER"
                                    } else {
                                        "UNKNOWN"
                                    }
                                };
                                // ignore access events
                                if !evt.kind.is_access() {
                                    self.log(
                                        LogType::Debug,
                                        LOG_WHEN_PROC,
                                        LOG_STATUS_OK,
                                        &format!("event notification caught: {evt_s}"),
                                    );
                                    match self.fire_condition() {
                                        Ok(_) => {
                                            self.log(
                                                LogType::Debug,
                                                LOG_WHEN_PROC,
                                                LOG_STATUS_OK,
                                                "condition fired successfully",
                                            );
                                        }
                                        Err(e) => {
                                            self.log(
                                                LogType::Warn,
                                                LOG_WHEN_PROC,
                                                LOG_STATUS_FAIL,
                                                &format!("error firing condition: {e}"),
                                            );
                                        }
                                    }
                                } else {
                                    self.log(
                                        LogType::Debug,
                                        LOG_WHEN_PROC,
                                        LOG_STATUS_OK,
                                        &format!("non-change event notification caught: {evt_s}"),
                                    );
                                }
                            }
                            Err(e) => {
                                self.log(
                                    LogType::Warn,
                                    LOG_WHEN_PROC,
                                    LOG_STATUS_FAIL,
                                    &format!("error in event notification: {e}"),
                                );
                            }
                        };
                    }
                    TargetOrQuitEvent::QuitError => {
                        self.log(
                            LogType::Warn,
                            LOG_WHEN_END,
                            LOG_STATUS_FAIL,
                            "request to quit generated an error: exiting anyway",
                        );
                        break;
                    }
                    TargetOrQuitEvent::Quit => {
                        self.log(
                            LogType::Debug,
                            LOG_WHEN_END,
                            LOG_STATUS_OK,
                            "event listener termination request caught",
                        );
                        break;
                    }
                }
            }
        }); // futures::executor::block_on(...)

        // as said above this should be ininfluent
        let _ = _quit_handle.join();

        // declare that the event has stopped
        self.log(
            LogType::Debug,
            LOG_WHEN_END,
            LOG_STATUS_OK,
            "stopping file change watch service",
        );

        let mut running = self.thread_running.write().unwrap();
        *running = false;
        Ok(true)
    }

    fn stop_service(&self) -> Result<bool> {
        match self.thread_running.read() {
            Ok(running) => {
                if *running {
                    self.log(
                        LogType::Debug,
                        LOG_WHEN_END,
                        LOG_STATUS_OK,
                        "the listener has been requested to stop",
                    );
                    // send the quit signal
                    let quit_tx = self.quit_tx.clone();
                    if let Some(tx) = quit_tx {
                        tx.clone().send(()).unwrap();
                        Ok(true)
                    } else {
                        self.log(
                            LogType::Warn,
                            LOG_WHEN_END,
                            LOG_STATUS_OK,
                            "impossible to contact the listener: stop request dropped",
                        );
                        Ok(false)
                    }
                } else {
                    self.log(
                        LogType::Debug,
                        LOG_WHEN_END,
                        LOG_STATUS_OK,
                        "the listener is not running: stop request dropped",
                    );
                    Ok(false)
                }
            }
            _ => {
                self.log(
                    LogType::Error,
                    LOG_WHEN_END,
                    LOG_STATUS_ERR,
                    "could not determine whether the listener is running",
                );
                Err(Error::new(
                    Kind::Forbidden,
                    ERR_EVENT_LISTENING_NOT_DETERMINED,
                ))
            }
        }
    }

    // this function is a wrapper for the actual asynchronous event receiver
    async fn stel_event_triggered(&mut self) -> Result<Option<String>> {
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

    fn stel_prepare_listener(&mut self) -> Result<bool> {
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
                            p.as_os_str().to_string_lossy()
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
                            p.as_os_str().to_string_lossy()
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

    fn thread_running(&self) -> Result<bool> {
        match self.thread_running.read() {
            Ok(running) => Ok(*running),
            _ => Err(Error::new(
                Kind::Forbidden,
                ERR_EVENT_LISTENING_NOT_DETERMINED,
            )),
        }
    }
}

// end.
