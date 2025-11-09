//! Define a WMI query based event

#![cfg(windows)]
#![cfg(feature = "wmi")]

use futures::Stream;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::{RwLock, mpsc};
use std::thread;

use futures::{FutureExt, SinkExt, StreamExt, channel::mpsc::channel, pin_mut, select};

use cfgmap::CfgMap;

use async_std::task;

use wmi::{IWbemClassWrapper, WMIConnection, WMIResult};

use super::base::Event;
use crate::common::logging::{LogType, log};
use crate::common::wres::{Error, Kind, Result};
use crate::condition::bucket_cond::ExecutionBucket;
use crate::condition::registry::ConditionRegistry;
use crate::{cfg_mandatory, constants::*};

use crate::cfghelp::*;

/// WMI Based Event
///
/// Implements an event based upon WMI suscription to certain events, using
/// the [wmi](https://docs.rs/wmi/latest/wmi/) Windows-targeted WMI library.
///
/// **Note**: the `match_query` holds a string implementing the *WMI query*:
/// see https://learn.microsoft.com/en-us/windows/win32/wmisdk/receiving-a-wmi-event
pub struct WmiQueryEvent {
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
    namespace: Option<String>,
    match_query: Option<String>,

    // internal values
    thread_running: RwLock<bool>,
    quit_tx: Option<mpsc::Sender<()>>,
}

impl Hash for WmiQueryEvent {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // common part
        self.event_name.hash(state);
        if let Some(s) = &self.condition_name {
            s.hash(state);
        }

        // specific part
        // 0 is hashed on the else branch in order to avoid that adjacent
        // strings one of which is undefined allow for hash collisions
        if let Some(x) = &self.match_query {
            x.hash(state);
        } else {
            0.hash(state);
        }

        if let Some(x) = &self.namespace {
            x.hash(state);
        } else {
            0.hash(state);
        }
    }
}

// implement cloning
impl Clone for WmiQueryEvent {
    fn clone(&self) -> Self {
        WmiQueryEvent {
            // reset ID
            event_id: 0,

            // parameters
            event_name: self.event_name.clone(),
            condition_name: self.condition_name.clone(),

            // internal values
            condition_registry: None,
            condition_bucket: None,

            // specific members
            // parameters
            namespace: self.namespace.clone(),
            match_query: self.match_query.clone(),

            // internal values
            thread_running: RwLock::new(false),
            quit_tx: None,
        }
    }
}

#[allow(dead_code)]
impl WmiQueryEvent {
    /// Create a new `WmiQueryEvent` with the provided name
    pub fn new(name: &str) -> Self {
        log(
            LogType::Debug,
            LOG_EMITTER_EVENT_WMI,
            LOG_ACTION_NEW,
            Some((name, 0)),
            LOG_WHEN_INIT,
            LOG_STATUS_MSG,
            &format!("EVENT {name}: creating a new WMI query based event"),
        );
        WmiQueryEvent {
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
            namespace: None,
            match_query: None,

            // internal values
            thread_running: RwLock::new(false),
            quit_tx: None,
        }
    }

    /// Set the match query to the provided value
    pub fn set_match_query(&mut self, query: &str) -> bool {
        self.match_query = Some(String::from(query));
        true
    }

    /// Return an owned copy of the match query
    pub fn match_query(&self) -> Option<String> {
        self.match_query.clone()
    }

    /// Set the namespace to the provided value
    pub fn set_namespace(&mut self, ns: &str) -> bool {
        self.namespace = Some(String::from(ns));
        true
    }

    /// Return an owned copy of the namespace
    pub fn namespace(&self) -> Option<String> {
        self.namespace.clone()
    }

    /// Load a `WmiQueryEvent` from a [`CfgMap`](https://docs.rs/cfgmap/latest/)
    ///
    /// The `WmiQueryEvent` is initialized according to the values
    /// provided in the `CfgMap` argument. If the `CfgMap` format does not
    /// comply with the requirements of a `WmiQueryEvent` an error is
    /// raised.
    pub fn load_cfgmap(
        cfgmap: &CfgMap,
        cond_registry: &'static ConditionRegistry,
        bucket: &'static ExecutionBucket,
    ) -> Result<WmiQueryEvent> {
        let check = vec!["type", "name", "tags", "condition", "namespace", "query"];
        cfg_check_keys(cfgmap, &check)?;

        // common mandatory parameter retrieval

        // type and name are both mandatory but type is only checked
        cfg_mandatory!(cfg_string_check_exact(cfgmap, "type", "wmi"))?;
        let name = cfg_mandatory!(cfg_string_check_regex(cfgmap, "name", &RE_EVENT_NAME))?.unwrap();

        // specific mandatory parameter initialization
        let query = cfg_mandatory!(cfg_string(cfgmap, "query"))?.unwrap();

        // initialize the structure
        // NOTE: the value of "event" for the condition type, which is
        //       completely functionally equivalent to "bucket", can only
        //       be set from the configuration file; programmatically built
        //       conditions of this type will only report "bucket" as their
        //       type, and "event" is only left for configuration readability
        let mut new_event = WmiQueryEvent::new(&name);
        new_event.condition_registry = Some(cond_registry);
        new_event.condition_bucket = Some(bucket);
        new_event.match_query = Some(query);

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
        if let Some(v) = cfg_string_check_regex(cfgmap, "namespace", &RE_WMI_NAMESPACE)? {
            new_event.namespace = Some(v.replace("/", "\\"));
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
        let check = vec!["type", "name", "tags", "condition", "namespace", "query"];
        cfg_check_keys(cfgmap, &check)?;

        // common mandatory parameter retrieval

        // type and name are both mandatory: type is checked and name is kept
        cfg_mandatory!(cfg_string_check_exact(cfgmap, "type", "wmi"))?;
        let name = cfg_mandatory!(cfg_string_check_regex(cfgmap, "name", &RE_EVENT_NAME))?.unwrap();
        cfg_mandatory!(cfg_string(cfgmap, "query"))?;

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
        cfg_string_check_regex(cfgmap, "namespace", &RE_WMI_NAMESPACE)?;

        Ok(name)
    }
}

impl Event for WmiQueryEvent {
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

    fn assign_quit_sender(&mut self, sr: mpsc::Sender<()>) {
        assert!(
            self.get_id() != 0,
            "event {} not registered",
            self.get_name()
        );
        self.quit_tx = Some(sr);
    }

    fn run_service(&self, qrx: Option<mpsc::Receiver<()>>) -> Result<bool> {
        assert!(
            qrx.is_some(),
            "quit signal channel receiver must be provided"
        );
        assert!(
            self.quit_tx.is_some(),
            "quit signal channel transmitter not initialized"
        );

        // unified event type that will be sent over an async channel by
        // either a `quit` command or the watcher: the `Target` option
        // ignores the event generated by the event subscription query;
        // the raw async query executor (`exec_notification_query_async`) is
        // used instead of higher level utilities, because the filters can
        // be defined in the query, directly, for maximum flexibility, and
        // therefore we do not really care about the event payload
        enum TargetOrQuitEvent {
            Target,
            Quit,
            QuitError,
        }

        // in this stream reader the event payload is skipped for the above
        // explained reason, and because it would be a mess to pass such
        // payload across threads
        async fn _get_wmi_event<T>(stream: &mut T) -> Option<TargetOrQuitEvent>
        where
            T: Stream<Item = WMIResult<IWbemClassWrapper>> + Unpin,
        {
            if let Some(m) = stream.next().await {
                if m.is_ok() {
                    Some(TargetOrQuitEvent::Target)
                } else {
                    None
                }
            } else {
                None
            }
        }

        // this function is built only for symmetry, in order to make clear
        // what is selected in the `select!` block within the async loop
        async fn _get_quit_message(
            rx: &mut futures::channel::mpsc::Receiver<TargetOrQuitEvent>,
        ) -> Option<TargetOrQuitEvent> {
            rx.next().await
        }

        // build an async communication channel for the quit signal
        let (aquit_tx, mut aquit_rx) = channel(EVENT_QUIT_CHANNEL_SIZE);

        // now it is time to set the internal `running` flag, before the
        // thread that waits for the quit signal is launched
        let mut running = self.thread_running.write().unwrap();
        *running = true;
        drop(running);

        // spawn a thread that only listens to a possible request to quit:
        // this thread should be lightweight enough, as it just waits all
        // the time; it is also useless to join to because it dies as soon
        // as it catches a signal
        let mut aq_tx_clone = aquit_tx.clone();
        let _quit_handle = thread::spawn(move || {
            match qrx.unwrap().recv() {
                Ok(_) => {
                    // send a quit message over the async channel
                    task::block_on({
                        async move {
                            aq_tx_clone.send(TargetOrQuitEvent::Quit).await.unwrap();
                        }
                    });
                }
                _ => {
                    // in case of error, send just the error option of the enum
                    task::block_on({
                        async move {
                            aq_tx_clone
                                .send(TargetOrQuitEvent::QuitError)
                                .await
                                .unwrap();
                        }
                    });
                }
            };
        });

        // the following are to enable the execution of a WMI async query
        let conn = if self.namespace.is_some() {
            WMIConnection::with_namespace_path(self.namespace.as_ref().unwrap())?
        } else {
            WMIConnection::new()?
        };

        let query = self.match_query.clone().unwrap();
        let mut wmi_stream = conn.exec_notification_query_async(query)?;
        self.log(
            LogType::Debug,
            LOG_WHEN_START,
            LOG_STATUS_OK,
            "successfully subscribed to WMI event",
        );

        // this should run in the local pool
        futures::executor::block_on(async move {
            'outer: loop {
                // wait on either one of the two possible messages
                let fwmi = _get_wmi_event(&mut wmi_stream).fuse();
                let fquit = _get_quit_message(&mut aquit_rx).fuse();
                pin_mut!(fwmi, fquit);
                let nextevent = select! {
                    mw = fwmi => mw,
                    mq = fquit => mq,
                };

                // for how the WMI event is handled here, the handler is very
                // simple as the appearance of the `Target` variant of the
                // `TargetOrQuitEvent` just states that the event took place
                if let Some(toq) = nextevent {
                    match toq {
                        TargetOrQuitEvent::QuitError => {
                            self.log(
                                LogType::Warn,
                                LOG_WHEN_PROC,
                                LOG_STATUS_FAIL,
                                "request to quit generated an error: exiting anyway",
                            );
                            break 'outer;
                        }
                        TargetOrQuitEvent::Quit => {
                            self.log(
                                LogType::Debug,
                                LOG_WHEN_END,
                                LOG_STATUS_OK,
                                "event listener termination request caught",
                            );
                            break 'outer;
                        }
                        TargetOrQuitEvent::Target => {
                            // in this case the event has been caught and
                            // this only causes the condition to fire, no
                            // further tests are performed because there
                            // is no event payload to be tested
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
                                }
                            }
                        }
                    }
                }
            }
        });

        // as said above this should be ininfluent
        let _ = _quit_handle.join();

        self.log(
            LogType::Debug,
            LOG_WHEN_END,
            LOG_STATUS_OK,
            "closing WMI event listening service",
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
                        "the service is not running: stop request dropped",
                    );
                    Ok(false)
                }
            }
            _ => {
                self.log(
                    LogType::Error,
                    LOG_WHEN_END,
                    LOG_STATUS_ERR,
                    "could not determine whether the service is running",
                );
                Err(Error::new(
                    Kind::Forbidden,
                    ERR_EVENT_LISTENING_NOT_DETERMINED,
                ))
            }
        }
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
