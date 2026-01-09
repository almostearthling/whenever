//! # Event Registry
//!
//! `event::registry` implements the main registry for `Event` objects.
//!
//! Implements the event registry which is created as the static repository of
//! all the events that are listened for in the main program.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::RwLock;
use std::thread;
use std::thread::JoinHandle;

use futures;
use futures::SinkExt;
use futures::future::select_all;
use futures::stream::StreamExt;
use futures::{future::FutureExt, pin_mut, select};

use lazy_static::lazy_static;
use unique_id::Generator;
use unique_id::sequence::SequenceGenerator;

use super::base::{Event, EventRef};
use crate::common::logging::{LogType, log};
use crate::common::wres::{Error, Kind, Result};
use crate::constants::*;

// module-wide values
lazy_static! {
    // the main event ID generator
    static ref UID_GENERATOR: SequenceGenerator = {
        SequenceGenerator
    };
}

// the specific event ID generator: used internally to register an event
#[allow(dead_code)]
fn generate_event_id() -> i64 {
    UID_GENERATOR.next_id()
}

/// The event registry: there must be one and only one event registry in each
/// instance of the process, and should have `'static` lifetime. It may be
/// passed around as a reference for events.
pub struct EventRegistry {
    // the entire map is synchronized in order to avoid concurrent access
    events: Arc<Mutex<HashMap<String, EventRef>>>,

    // the triggerable list is kept separate because the triggerable
    // attribute is actually a constant that can be retrieved at startup
    // and we do not want to be blocked while directly asking an active
    // event on its ability to be manually triggered
    triggerable_events: RwLock<HashMap<String, bool>>,

    // the channel over which a request to stop the listener can be sent
    listener_quit_messenger: Arc<Mutex<Option<futures::channel::mpsc::Sender<()>>>>,

    // the service handle for the event listener
    listener_handle: Arc<Mutex<Option<JoinHandle<Result<bool>>>>>,
}

#[allow(dead_code)]
impl EventRegistry {
    /// Create a new, empty `EventRegistry`.
    pub fn new() -> Self {
        EventRegistry {
            events: Arc::new(Mutex::new(HashMap::new())),
            // event_waiters: Arc::new(Mutex::new(HashMap::new())),
            triggerable_events: RwLock::new(HashMap::new()),
            listener_quit_messenger: Arc::new(Mutex::new(None)),
            listener_handle: Arc::new(Mutex::new(None)),
        }
    }

    /// Run the main event listener
    pub fn run_event_listener(&'static self) -> Result<()> {
        // this enum is a message that either reports the outcome of a
        // triggered event or the instruction to stop listening
        enum TriggeredOrQuitMessage {
            Triggered(Result<Option<String>>),
            Quit,
            QuitError,
        }

        // simplify collection of ToQMs sent by events, that is, just Targets
        // via an async function that builds the Target message for us
        async fn next_event(registry: Arc<Mutex<&EventRegistry>>) -> TriggeredOrQuitMessage {
            let r0 = registry.clone();
            let r0 = r0.lock().expect("cannot lock event registry");
            let el0 = r0.events.clone();
            drop(r0);

            // WARNING: the event list is acquired and locked here, this means
            // that it cannot be modified while we are waiting for any event
            // to occcur; this can be a problem when the list should changed,
            // generally because of a reconfiguration, however the listener is
            // stopped and restarted in that case, this should limit problems;
            // also, check that the list of futures is not empty (which would
            // cause a panic), and if empty return None as data, which is just
            // a no-op in the event poller
            let mut el0 = el0.lock().expect("cannot lock event list");
            if el0.is_empty() {
                TriggeredOrQuitMessage::Triggered(Ok(None))
            } else {
                let catch_events = el0.iter_mut().map(|(_, evt)| evt.event_triggered());

                // only the first item of the tuple is needed for our purposes
                let res = select_all(catch_events).await;
                TriggeredOrQuitMessage::Triggered(res.0)
            }
        }

        // simplify collection of ToQMs sent through `listener_quit_messenger`
        // via an async function that directly returns the `Quit` signal when
        // it receives something
        async fn next_quit(mut rx: futures::channel::mpsc::Receiver<()>) -> TriggeredOrQuitMessage {
            if rx.next().await.is_some() {
                TriggeredOrQuitMessage::Quit
            } else {
                TriggeredOrQuitMessage::QuitError
            }
        }

        log(
            LogType::Debug,
            LOG_EMITTER_EVENT_REGISTRY,
            LOG_ACTION_MAIN_LISTENER,
            None,
            LOG_WHEN_START,
            LOG_STATUS_OK,
            "main event listener is starting",
        );

        let managed_registry = Arc::new(Mutex::new(self));

        // create the stream used to send the quit message and assign it to the registry
        let (qtx, qrx) = futures::channel::mpsc::channel::<()>(EVENT_QUIT_CHANNEL_SIZE);
        let r0 = managed_registry.lock().expect("cannot lock event registry");
        let m0 = r0.listener_quit_messenger.clone();
        let mut m1 = m0
            .lock()
            .expect("cannot lock quit messenger for initialization");
        *m1 = Some(qtx);
        drop(m1);
        drop(m0);
        drop(r0);

        // this is needed to allow for saving the service handle below
        let registry = managed_registry.clone();

        // the only existing service thread, which listens for both events
        // and quit messages: no other threads are spawned in this version
        let handle = thread::spawn(move || {
            let r0 = registry.clone();
            let r0 = r0.lock().expect("cannot lock event registry");
            let el0 = r0.events.clone();
            let mut el0 = el0.lock().expect("cannot lock event list");

            for (name, event) in el0.iter_mut() {
                if !event.initial_setup()? {
                    log(
                        LogType::Trace,
                        LOG_EMITTER_EVENT_REGISTRY,
                        LOG_ACTION_INSTALL,
                        None,
                        LOG_WHEN_INIT,
                        LOG_STATUS_MSG,
                        &format!("initialization skipped for event {name}",),
                    );
                } else {
                    log(
                        LogType::Trace,
                        LOG_EMITTER_EVENT_REGISTRY,
                        LOG_ACTION_INSTALL,
                        None,
                        LOG_WHEN_INIT,
                        LOG_STATUS_MSG,
                        &format!("event {name} successfully initialized",),
                    );
                }
            }
            drop(el0);
            drop(r0);

            log(
                LogType::Trace,
                LOG_EMITTER_EVENT_REGISTRY,
                LOG_ACTION_MAIN_LISTENER,
                None,
                LOG_WHEN_START,
                LOG_STATUS_OK,
                "the main event listener now handles events",
            );

            // the quit listener is used only once in this function, therefore
            // it is safe to be created outside the loop; in fact, since the
            // main listener is stopped and restarted on reconfiguration, this
            // thread also will stop and restart, and the quit signal listener
            // will be recreated from scratch
            let wait_quit = next_quit(qrx).fuse();
            pin_mut!(wait_quit);

            let r0 = registry.clone();

            // do our loop, which can only be terminated by an explicit signal
            futures::executor::block_on(async move {
                loop {
                    // the future that waits for events is created within the
                    // loop in order to keep it alive as long as the loop
                    // runs, that is, until a quit signal is received
                    let r1 = r0.clone();
                    let wait_event = next_event(r1).fuse();
                    pin_mut!(wait_event);

                    select! {
                        m = wait_event => {
                            match m {
                                TriggeredOrQuitMessage::Triggered(res) => {
                                    if let Ok(res) = res {
                                        if let Some(name) = res {
                                            log(
                                                LogType::Trace,
                                                LOG_EMITTER_EVENT_REGISTRY,
                                                LOG_ACTION_MAIN_LISTENER,
                                                None,
                                                LOG_WHEN_PROC,
                                                LOG_STATUS_OK,
                                                &format!("event {name} has been triggered"),
                                            );
                                        }
                                    } else {
                                        log(
                                            LogType::Trace,
                                            LOG_EMITTER_EVENT_REGISTRY,
                                            LOG_ACTION_MAIN_LISTENER,
                                            None,
                                            LOG_WHEN_PROC,
                                            LOG_STATUS_ERR,
                                            &format!(
                                                "an error occurred: `{}`",
                                                res.unwrap_err(),
                                            ),
                                        );
                                    }
                                },
                                // this can never happen, as the next_event
                                // function only returns `Triggered(data)`
                                _ => unreachable!(),
                            }
                        },
                        m = wait_quit => {
                            match m {
                                TriggeredOrQuitMessage::Quit => {
                                    log(
                                        LogType::Trace,
                                        LOG_EMITTER_EVENT_REGISTRY,
                                        LOG_ACTION_MAIN_LISTENER,
                                        None,
                                        LOG_WHEN_PROC,
                                        LOG_STATUS_OK,
                                        "request received to stop the event listener",
                                    );
                                    break;
                                },
                                TriggeredOrQuitMessage::QuitError => {
                                    log(
                                        LogType::Trace,
                                        LOG_EMITTER_EVENT_REGISTRY,
                                        LOG_ACTION_MAIN_LISTENER,
                                        None,
                                        LOG_WHEN_PROC,
                                        LOG_STATUS_OK,
                                        "error receiving a request to stop the event listener: exiting anyway",
                                    );
                                    break;
                                },
                                // this can never happen, as the next_quit
                                // function only returns `Quit` or `QuitError`
                                _ => unreachable!(),
                            }
                        },
                    }
                }
            });

            let r0 = registry.clone();
            let r0 = r0.lock().expect("cannot lock event registry");
            let el0 = r0.events.clone();
            let mut el0 = el0.lock().expect("cannot lock event list");

            for (name, event) in el0.iter_mut() {
                if !event.final_cleanup()? {
                    log(
                        LogType::Trace,
                        LOG_EMITTER_EVENT_REGISTRY,
                        LOG_ACTION_INSTALL,
                        None,
                        LOG_WHEN_END,
                        LOG_STATUS_MSG,
                        &format!("cleanup skipped for event {name}",),
                    );
                } else {
                    log(
                        LogType::Trace,
                        LOG_EMITTER_EVENT_REGISTRY,
                        LOG_ACTION_INSTALL,
                        None,
                        LOG_WHEN_END,
                        LOG_STATUS_MSG,
                        &format!("event {name} successfully cleaned up",),
                    );
                }
            }
            drop(el0);
            drop(r0);

            log(
                LogType::Debug,
                LOG_EMITTER_EVENT_REGISTRY,
                LOG_ACTION_MAIN_LISTENER,
                None,
                LOG_WHEN_END,
                LOG_STATUS_OK,
                "main event listener stopped",
            );
            Ok(true)
        });

        let managed_registry = managed_registry.clone();
        let managed_registry = managed_registry.lock().expect("cannot lock event registry");

        let h0 = managed_registry.listener_handle.clone();
        let mut h0 = h0.lock().unwrap();
        *h0 = Some(handle);
        drop(h0);

        Ok(())
    }

    /// Stop the main event listener
    pub fn stop_event_listener(&'static self) -> Result<bool> {
        let managed_registry = Arc::new(Mutex::new(self));

        let m0 = managed_registry.clone();
        let m0 = m0.lock().expect("cannot acquire event registry");
        let messenger = m0.listener_quit_messenger.clone();
        drop(m0);
        let mut messenger = messenger.lock().expect("cannot acquire listener messenger");
        if messenger.is_some() {
            log(
                LogType::Trace,
                LOG_EMITTER_EVENT_REGISTRY,
                LOG_ACTION_MAIN_LISTENER,
                None,
                LOG_WHEN_START,
                LOG_STATUS_MSG,
                "requesting the event listener to stop",
            );
            let messenger = messenger.as_mut().unwrap();
            futures::executor::block_on(async move {
                if messenger.send(()).await.is_err() {
                    log(
                        LogType::Debug,
                        LOG_EMITTER_EVENT_REGISTRY,
                        LOG_ACTION_MAIN_LISTENER,
                        None,
                        LOG_WHEN_END,
                        LOG_STATUS_FAIL,
                        "an error occurred while requesting to stop the event listener",
                    );
                }
            });
        }

        // take ownership of the service handle and replace it with `None`
        let m0 = managed_registry.clone();
        let m0 = m0.lock().expect("cannot lock event registry");
        let h0 = m0.listener_handle.clone();
        let mut h0 = h0.lock().unwrap();

        let handle = h0.take();
        let res = handle.unwrap().join();
        if res.is_err() {
            log(
                LogType::Debug,
                LOG_EMITTER_EVENT_REGISTRY,
                LOG_ACTION_MAIN_LISTENER,
                None,
                LOG_WHEN_END,
                LOG_STATUS_ERR,
                "an error occurred while stopping the event listener",
            );
            return Err(Error::new(Kind::Failed, ERR_EVENTREG_CANNOT_STOP_LISTENER));
        }
        res.unwrap()
    }

    /// Check whether or not an event with the provided name is in the
    /// registry.
    ///
    /// # Arguments
    ///
    /// * name - the name of the event to check for registration
    ///
    /// # Panics
    ///
    /// May panic if the event registry could not be locked for enquiry.
    pub fn has_event(&self, name: &str) -> bool {
        self.events
            .clone()
            .lock()
            .expect("cannot lock event registry")
            .contains_key(name)
    }

    /// Check whether or not the provided event is in the registry.
    ///
    /// # Arguments
    ///
    /// * event - the reference to an event to check for registration
    ///
    /// # Panics
    ///
    /// May panic if the event registry could not be locked for enquiry
    /// or the contained event cannot be locked for comparison.
    pub fn has_event_eq(&self, event: &dyn Event) -> bool {
        let name = event.get_name();
        if self.has_event(name.as_str()) {
            let el0 = self.events.clone();
            let el0 = el0.lock().expect("cannot lock event registry");
            let found_event = el0.get(name.as_str()).unwrap();
            let equals = found_event.eq(event);
            return equals;
        }

        false
    }

    /// Add an already-boxed `Event` if its name is not present in the
    /// registry.
    ///
    /// The `Box` ensures that the enclosed event is transferred as a
    /// reference and stored as-is in the registry. Note that for the
    /// registration to be successful there must **not** already be an event
    /// with the same name in the registry: if such event is found
    /// `Ok(false)` is returned. In order to replace an `Event` it has to be
    /// removed first, then added.
    ///
    /// # Arguments
    ///
    /// * `boxed_event` - an object implementing the `base::Event` trait,
    ///   provided to the function as a `Box<dyn Event>` aka `EventRef`
    ///
    /// # Returns
    ///
    /// * `Ok(true)` - the event could be added to the registry
    /// * `Ok(false)` - the event could not be inserted
    ///
    /// **Note**: the event is _moved_ into the registry, and can only be
    /// released (and given back stored in a `Box`) using the `remove_event`
    /// function. Also, although the possible outcomes include an error
    /// condition, `Err(_)` is never returned.
    ///
    /// # Panics
    ///
    /// May panic if the event registry could not be locked for insertion.
    pub fn add_event(&self, mut boxed_event: EventRef) -> Result<bool> {
        let name = boxed_event.get_name();
        if self.has_event(&name) {
            return Ok(false);
        }
        // only consume an ID if the event is not discarded, otherwise the
        // released event would be safe to use even when not registered
        boxed_event.set_id(generate_event_id());
        self.triggerable_events
            .write()
            .expect("cannot write to triggerable event registry")
            .insert(name.clone(), boxed_event.triggerable());
        self.events
            .clone()
            .lock()
            .expect("cannot lock event registry")
            .insert(name, boxed_event);
        Ok(true)
    }

    /// Remove a named event from the list and give it back stored in a Box.
    ///
    /// The returned `Event` can be modified and stored back in the
    /// registry: before returning, the boxed `Event` is 'uninitialized'
    /// (that is, its ID is set back to 0) so that it wouldn't be checked if
    /// asked to; the rest of its internal status is preserved.
    ///
    /// **Note**: this function should be called on events whose listening
    /// service is not running, which can also be checked using the registry
    /// API instead of directly inspecting the event; in fact the event
    /// service manager should be the only utility using the removal function
    /// although it has the drawback of completely dropping the event.
    ///
    /// # Arguments
    ///
    /// * `name` - the name of the event that must be removed
    ///
    /// # Returns
    ///
    /// * `Error(Kind::Failed, _)` - the event could not be removed
    /// * `Ok(None)` - condition not found in registry
    /// * `Ok(Event)` - the removed (_pulled out_) `Event` on success
    ///
    /// # Panics
    ///
    /// May panic if the event registry could not be locked for extraction,
    /// or if an attempt is made to extract an event that is in use (FIXME:
    /// maybe it should return an error in this case?).
    pub fn remove_event(&self, name: &str) -> Result<Option<EventRef>> {
        if self.has_event(name) {
            match self
                .events
                .clone()
                .lock()
                .expect("cannot lock event registry")
                .remove(name)
            {
                Some(e) => {
                    // in this case if the event cannot be extracted from the list
                    // no reference to the event is returned, but an error instead
                    // WARNING: the reference is dropped in that case!
                    // let e = Arc::try_unwrap(e);
                    // let Ok(event) = e else {
                    //     return Err(Error::new(Kind::Failed, ERR_EVENTREG_CANNOT_PULL_EVENT));
                    // };
                    // let mut event = event.into_inner().expect("cannot extract locked event");
                    let mut event = e;
                    event.set_id(0);
                    Ok(Some(event))
                }
                _ => Err(Error::new(Kind::Failed, ERR_EVENTREG_CANNOT_REMOVE_EVENT)),
            }
        } else {
            Ok(None)
        }
    }

    /// Return the list of event names as owned strings.
    ///
    /// Return a vector containing the names of all the events that have been
    /// registered, as `String` elements.
    ///
    /// # Panics
    ///
    /// May panic if the event registry could not be locked for extraction.
    pub fn event_names(&self) -> Option<Vec<String>> {
        let mut res = Vec::new();

        for name in self
            .events
            .clone()
            .lock()
            .expect("cannot lock event registry")
            .keys()
        {
            res.push(name.clone())
        }
        if res.is_empty() { None } else { Some(res) }
    }

    /// Return the id of the specified event.
    pub fn event_id(&self, name: &str) -> Option<i64> {
        if self.has_event(name) {
            let el0 = self.events.clone();
            let el0 = el0.lock().expect("cannot lock event registry");
            let event = el0.get(name).expect("cannot retrieve event");
            let id = event.get_id();
            Some(id)
        } else {
            None
        }
    }

    /// Tell whether or not an event is triggerable, `None` if event not found.
    pub fn event_triggerable(&self, name: &str) -> Option<bool> {
        if self.has_event(name) {
            let triggerable = *self
                .triggerable_events
                .read()
                .expect("cannot read triggerable event registry")
                .get(name)
                .unwrap();
            Some(triggerable)
        } else {
            None
        }
    }

    /// Trigger an event.
    ///
    /// If the event can be manually triggered, fire its associated condition
    /// and return the result of the call to the event `fire_condition()` call,
    /// otherwise return `Ok(false)`.
    ///
    /// # Panics
    ///
    /// When the event it is called upon is not registered: in no way this
    /// should be called for unregistered events.
    pub fn trigger_event(&self, name: &str) -> Result<bool> {
        assert!(self.has_event(name), "event {name} not in registry");
        assert!(
            self.event_triggerable(name).unwrap(),
            "event {name} cannot be manually triggered",
        );

        // what follows just *reads* the registry: the event is retrieved
        // and the corresponding structure is operated in a way that mutates
        // only its inner state, and not the wrapping pointer
        let id = self.event_id(name).unwrap();
        let el0 = self.events.clone();
        let el0 = el0.lock().expect("cannot lock event registry");
        let event = el0.get(name).expect("cannot retrieve event for triggering");

        log(
            LogType::Debug,
            LOG_EMITTER_EVENT_REGISTRY,
            LOG_ACTION_TRIGGER,
            Some((name, id)),
            LOG_WHEN_PROC,
            LOG_STATUS_OK,
            &format!("manually triggering event {name}"),
        );
        match event.fire_condition() {
            Ok(res) => {
                if res {
                    log(
                        LogType::Debug,
                        LOG_EMITTER_EVENT_REGISTRY,
                        LOG_ACTION_FIRE,
                        Some((name, id)),
                        LOG_WHEN_PROC,
                        LOG_STATUS_OK,
                        &format!("condition for event {name} fired"),
                    );
                } else {
                    log(
                        LogType::Debug,
                        LOG_EMITTER_EVENT_REGISTRY,
                        LOG_ACTION_FIRE,
                        Some((name, id)),
                        LOG_WHEN_PROC,
                        LOG_STATUS_FAIL,
                        &format!("condition for event {name} could not fire"),
                    );
                }
                Ok(res)
            }
            Err(e) => {
                log(
                    LogType::Debug,
                    LOG_EMITTER_EVENT_REGISTRY,
                    LOG_ACTION_FIRE,
                    Some((name, id)),
                    LOG_WHEN_PROC,
                    LOG_STATUS_FAIL,
                    &format!("condition for event {name} failed to fire"),
                );
                Err(e)
            }
        }
    }

    /// Fire the condition associated to the named event.
    ///
    /// This version calls in turn the events `fire_condition()` method, but
    /// has the advantage of being implemented on an object that has a
    /// `'static` lifetime.
    ///
    /// # Panics
    ///
    /// When the event it is called upon is not registered: in no way this
    /// should be called for unregistered events.
    fn fire_condition_for(&self, name: &str) {
        assert!(self.has_event(name), "event {name} not in registry");

        // what follows just *reads* the registry: the event is retrieved
        // and the corresponding structure is operated in a way that mutates
        // only its inner state, and not the wrapping pointer
        let id = self.event_id(name).unwrap();
        let el0 = self.events.clone();
        let el0 = el0.lock().expect("cannot lock event registry");
        let event = el0.get(name).expect("cannot retrieve event for activation");

        if let Ok(res) = event.fire_condition() {
            if res {
                log(
                    LogType::Trace,
                    LOG_EMITTER_EVENT_REGISTRY,
                    LOG_ACTION_FIRE,
                    Some((name, id)),
                    LOG_WHEN_PROC,
                    LOG_STATUS_OK,
                    &format!("condition for event {name} fired"),
                );
            } else {
                log(
                    LogType::Trace,
                    LOG_EMITTER_EVENT_REGISTRY,
                    LOG_ACTION_FIRE,
                    Some((name, id)),
                    LOG_WHEN_PROC,
                    LOG_STATUS_FAIL,
                    &format!("condition for event {name} could not fire"),
                );
            }
        } else {
            log(
                LogType::Debug,
                LOG_EMITTER_EVENT_REGISTRY,
                LOG_ACTION_FIRE,
                Some((name, id)),
                LOG_WHEN_PROC,
                LOG_STATUS_FAIL,
                &format!("condition for event {name} failed to fire"),
            );
        }
    }
}

// end.
