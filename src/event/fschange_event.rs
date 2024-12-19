//! Define event based on file/directory changes (aka _notify_)
//!
//! The user states to watch certain files or directories, and the OS emits
//! a notification everytime that a change occurs in the watched items. This
//! event puts an associated condition into the execution bucket each time
//! it happens.


use std::path::PathBuf;
use std::time::Duration;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::RwLock;

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll, Waker};
use std::sync::{Arc, Mutex};
use futures::channel::mpsc::{channel, Receiver};
use futures::{FutureExt, SinkExt, StreamExt, pin_mut};

use notify::{self, Watcher};
use cfgmap::CfgMap;

use super::base::Event;
use crate::condition::registry::ConditionRegistry;
use crate::condition::bucket_cond::ExecutionBucket;
use crate::common::logging::{log, LogType};
use crate::{cfg_mandatory, constants::*};

use crate::cfghelp::*;



// default seconds to wayt between active polls: generally ignored
const DEFAULT_FSCHANGE_POLL_SECONDS: u64 = 2;

// this is only to satisfy the channel
const MAX_FSEVENTS_ON_CHANNEL: usize = 1000;



// this is the async base to allow for the request to leave to be inquired
struct Quitter<'a> {
    shared_state: Arc<Mutex<SharedState<'a>>>,
}

struct SharedState<'a> {
    event: &'a FilesystemChangeEvent,
    waker: Option<Waker>,
}

impl Future for Quitter<'_> {
    type Output = ();
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut shared_state = self.shared_state.lock().unwrap();
        let quit = shared_state.event.must_exit.read().expect("cannot get event service quitting status");
        if *quit {
            Poll::Ready(())
        } else {
            shared_state.waker = Some(cx.waker().clone());
            Poll::Pending
        }
    }
}

impl <'a> Quitter <'a> {
    fn new(evt: &'a FilesystemChangeEvent) -> Self {
        let shared_state = Arc::new(Mutex::new(SharedState {
            event: evt,
            waker: None,
        }));

        Quitter { shared_state }
    }
}



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
    thread_running: RwLock<bool>,
    must_exit: RwLock<bool>,
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
            must_exit: RwLock::new(false),
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
            must_exit: RwLock::new(false),
        }
    }

    /// Add a location to be watched.
    pub fn watch_location(&mut self, location: &str) -> std::io::Result<bool> {
        let p = PathBuf::from(location);
        if p.exists() {
            self.log(
                LogType::Debug,
                LOG_WHEN_INIT,
                LOG_STATUS_OK,
                &format!("found valid item to watch: `{}`", p.as_os_str().to_string_lossy()),
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
                LOG_WHEN_INIT,
                LOG_STATUS_FAIL,
                &format!("refusing non-existent item: `{}`", p.as_os_str().to_string_lossy()),
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

        let check = vec![
            "type",
            "name",
            "tags",
            "condition",
            "watch",
            "recursive",
        ];
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
        let mut new_event = FilesystemChangeEvent::new(
            &name,
        );
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
    pub fn check_cfgmap(cfgmap: &CfgMap, available_conditions: &Vec<&str>) -> std::io::Result<String> {

        let check = vec![
            "type",
            "name",
            "tags",
            "condition",
            "watch",
            "recursive",
        ];
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
        cfg_vec_string(cfgmap, "watch")?;   // see above: we do not check for correctness
        cfg_bool(cfgmap, "recursive")?;
        cfg_int_check_above_eq(cfgmap, "poll_seconds", 0)?;

        Ok(name)
    }

}


impl Event for FilesystemChangeEvent {

    fn set_id(&mut self, id: i64) { self.event_id = id; }
    fn get_name(&self) -> String { self.event_name.clone() }
    fn get_id(&self) -> i64 { self.event_id }

    /// Return a hash of this item for comparison
    fn _hash(&self) -> u64 {
        let mut s = DefaultHasher::new();
        self.hash(&mut s);
        s.finish()
    }


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


    fn _run_service(&self) -> std::io::Result<bool> {

        async fn _wait_to_quit(evt: &FilesystemChangeEvent) {
            let quitter = Quitter::new(evt);
            quitter.await
        }

        // see https://github.com/notify-rs/notify/blob/main/examples/async_monitor.rs
        fn _build_async_watcher(cfg: notify::Config) -> notify::Result<(notify::RecommendedWatcher, Receiver<notify::Result<notify::Event>>)> {
            let (mut tx, rx) = channel(MAX_FSEVENTS_ON_CHANNEL);

            // Automatically select the best implementation for your platform.
            // You can also access each implementation directly e.g. INotifyWatcher.
            let watcher = notify::RecommendedWatcher::new(
                move |res| {
                    futures::executor::block_on(async {
                        tx.send(res).await.unwrap();
                    })
                },
                cfg,
            )?;

            Ok((watcher, rx))
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
        let notify_cfg = notify::Config::default()
            .with_poll_interval(Duration::from_secs(self.poll_seconds));

        // let (tx, rx) = std::sync::mpsc::channel();
        // let mut watcher = Box::new(
        //     notify::RecommendedWatcher::new(tx, notify_cfg).unwrap());

        let (mut watcher, mut rx) = _build_async_watcher(notify_cfg)
            .expect("could not create watcher");

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
                            LOG_WHEN_START,
                            LOG_STATUS_OK,
                            &format!("successfully added `{}` to watched paths", p.as_os_str().to_string_lossy()),
                        );
                    }
                    Err(e) => {
                        self.log(
                            LogType::Warn,
                            LOG_WHEN_START,
                            LOG_STATUS_FAIL,
                            &format!("could not add `{}` to watched paths: {e}", p.as_os_str().to_string_lossy()),
                        );
                    }
                }
            }
        } else {
            self.log(
                LogType::Error,
                LOG_WHEN_START,
                LOG_STATUS_FAIL,
                "no paths to watch have been specified",
            );
            return Ok(false);
        }

        // now that all possibilities to exit have gone it's time to set
        // the flag that states that the service thread is running
        if let Ok(mut running) = self.thread_running.write() {
            *running = true;
        }

        // the outer loop is async: the dedicated thread should block on it
        futures::executor::block_on(async {
            loop {
                let fs_event = rx.next().fuse();
                let quitting = _wait_to_quit(&self).fuse();

                pin_mut!(fs_event, quitting);

                // wait either for a watched file event or a request to quit
                futures::select! {
                    () = quitting => {
                        // if we are here, the quitter already found that this service must leave
                        self.log(
                            LogType::Debug,
                            LOG_WHEN_END,
                            LOG_STATUS_OK,
                            "event listener is stopping",
                        );
                        break;
                    }

                    r_evt = fs_event => {
                        if let Some(r_evt) = r_evt {
                            match r_evt {
                                Ok(evt) => {
                                    let evt_s = {
                                        if evt.kind.is_access() { "ACCESS" }
                                        else if evt.kind.is_create() { "CREATE" }
                                        else if evt.kind.is_modify() { "MODIFY" }
                                        else if evt.kind.is_remove() { "REMOVE" }
                                        else if evt.kind.is_other() { "OTHER" }
                                        else { "UNKNOWN" }
                                    };
                                    // ignore access events
                                    if !evt.kind.is_access() {
                                        self.log(
                                            LogType::Info,
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
                    }
                }
            };
        });

        self.log(
            LogType::Debug,
            LOG_WHEN_END,
            LOG_STATUS_OK,
            &format!("finished watching files for changes"),
        );
        if let Ok(mut running) = self.thread_running.write() {
            *running = false;
        }
        Ok(true)
    }

    fn _stop_service(&self) -> std::io::Result<bool> {
        if let Ok(running) = self.thread_running.read() {
            if *running {
                if let Ok(mut quit) = self.must_exit.write() {
                    *quit = true;
                    self.log(
                        LogType::Info,
                        LOG_WHEN_END,
                        LOG_STATUS_OK,
                        &format!("the listener has been requested to stop"),
                    );
                    Ok(true)
                } else {
                    self.log(
                        LogType::Error,
                        LOG_WHEN_END,
                        LOG_STATUS_ERR,
                        &format!("could not request the listener to stop"),
                    );
                    Err(std::io::Error::new(
                        std::io::ErrorKind::PermissionDenied,
                        "could not request the listener to stop"
                    ))
                }
            } else {
                self.log(
                    LogType::Trace,
                    LOG_WHEN_END,
                    LOG_STATUS_OK,
                    &format!("the listener is not running: stop request dropped"),
                );
                Ok(false)
            }
        } else {
            self.log(
                LogType::Error,
                LOG_WHEN_END,
                LOG_STATUS_ERR,
                &format!("could not determine whether the listener is running"),
            );
            Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "could not determine whether the listener is running"
            ))
        }
    }

    fn _thread_running(&self) -> std::io::Result<bool> {
        if let Ok(running) = self.thread_running.read() {
            Ok(*running)
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "could not determine whether the listener is running"
            ))
        }
    }

}


// end.
