//! # Event Registry
//!
//! `event::registry` implements the main registry for `Event` objects.
//!
//! Implements the event registry which is created as the static repository of
//! all events in the main program. This ensures that all the configured events
//! are instanced and have a lifetime that lasts for the whole time in which
//! the main program is running.


use std::sync::Arc;
use std::sync::Mutex;
use std::sync::RwLock;
use std::collections::HashMap;
use std::io::{Error, ErrorKind};
use std::thread;
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::Duration;

use lazy_static::lazy_static;
use unique_id::Generator;
use unique_id::sequence::SequenceGenerator;

use super::base::Event;
use crate::common::logging::{log, LogType};
use crate::constants::*;


// module-wide values
lazy_static! {
    // the main event ID generator
    static ref UID_GENERATOR: SequenceGenerator = {
        let mut _uidgen = SequenceGenerator;
        _uidgen
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
    // the entire list is enclosed in `RwLock<...>` in order to avoid
    // concurrent access to the list itself
    event_list: RwLock<HashMap<String, Arc<RwLock<Box<dyn Event>>>>>,

    // the triggerable list is kept separate because the triggerable
    // attribute is actually a constant that can be retrieved at startup
    // and we do not want to be blocked while directly asking an active
    // event on its ability to be manually triggered
    triggerable_event_list: RwLock<HashMap<String, bool>>,

    // the queues of events whose services need to be installed/removed
    event_service_install_queue: Arc<Mutex<Vec<String>>>,
    event_service_uninstall_queue: Arc<Mutex<Vec<String>>>,

    // flag to signal that the event service manager must exit
    event_service_manager_exiting: RwLock<bool>,
}


#[allow(dead_code)]
impl EventRegistry {

    /// Create a new, empty `EventRegistry`.
    pub fn new() -> Self {
        EventRegistry {
            event_list: RwLock::new(HashMap::new()),
            triggerable_event_list: RwLock::new(HashMap::new()),
            event_service_install_queue: Arc::new(Mutex::new(Vec::new())),
            event_service_uninstall_queue: Arc::new(Mutex::new(Vec::new())),
            event_service_manager_exiting: RwLock::new(false),
        }
    }


    /// Start the main registry thread, which in turn handles all other
    /// event listener threads
    // NOTE: this function should really be implemented using async, because
    // almost all operations are performed on the registry, which in turn
    // exactly what it's requested to do in every moment, thus is also able
    // to intentionally advance progress on the implemented futures
    pub fn run_event_service_manager(registry: &'static Self) -> Result<JoinHandle<Result<bool, std::io::Error>>, std::io::Error> {
        // self can be expected to be &'static mut because we know that this
        // registry lives as much as the entire program instance lives
        let rest_time = Duration::from_millis(MAIN_EVENT_REGISTRY_MGMT_MILLISECONDS);
        let registry = Arc::new(Mutex::new(Box::new(registry)));
        let mut service_handles: HashMap<String, JoinHandle<Result<bool, Error>>> = HashMap::new();
        let mut cleanup_events: Vec<String> = Vec::new();

        let _handle = thread::spawn(move || {
            log(
                LogType::Debug,
                LOG_EMITTER_EVENT_REGISTRY,
                LOG_ACTION_MAIN_LISTENER,
                None,
                LOG_WHEN_START,
                LOG_STATUS_OK,
                "starting main event service manager",
            );
            loop {
                let r0 = registry.clone();
                let uninstall_queue = r0
                    .lock()
                    .unwrap()
                    .event_service_uninstall_queue
                    .clone();
                drop(r0);
                let lq = uninstall_queue
                    .lock()
                    .expect("cannot lock event service uninstall queue");
                let names = lq.clone();
                drop(lq);
                for name in names {
                    let r1 = registry.clone();
                    let id = r1.lock().unwrap().event_id(&name).unwrap();
                    if r1.lock().unwrap().uninstall_event_service(&name).is_ok() {
                        log(
                            LogType::Trace,
                            LOG_EMITTER_EVENT_REGISTRY,
                            LOG_ACTION_UNINSTALL,
                            Some((&name, id)),
                            LOG_WHEN_PROC,
                            LOG_STATUS_OK,
                            "stopped handling listener",
                        );
                        cleanup_events.push(name);
                    } else {
                        log(
                            LogType::Trace,
                            LOG_EMITTER_EVENT_REGISTRY,
                            LOG_ACTION_UNINSTALL,
                            Some((&name, id)),
                            LOG_WHEN_PROC,
                            LOG_STATUS_FAIL,
                            "could not stop handling listener",
                        );
                    };
                }
                uninstall_queue.clone().lock().expect("cannot lock event service uninstall queue").clear();
                drop(uninstall_queue);

                let r0 = registry.clone();
                let install_queue = r0
                    .lock()
                    .unwrap()
                    .event_service_install_queue
                    .clone();
                drop(r0);
                let lq = install_queue
                    .lock()
                    .expect("cannot lock event service install queue");
                let names = lq.clone();
                drop(lq);
                for name in names {
                    // if an event has been modified and is still waiting to
                    // be removed from the registry, it is not possible to
                    // install its listener over the old one: skip and wait
                    // for it to be removed
                    if !cleanup_events.contains(&name) {
                        let r1 = registry.clone();
                        let id = r1.lock().unwrap().event_id(&name).unwrap();
                        if let Ok(o) = r1.lock().unwrap().install_event_service(&name) {
                            if let Some(service) = o {
                                service_handles.insert(name.clone(), service);
                                log(
                                    LogType::Trace,
                                    LOG_EMITTER_EVENT_REGISTRY,
                                    LOG_ACTION_INSTALL,
                                    Some((&name, id)),
                                    LOG_WHEN_PROC,
                                    LOG_STATUS_OK,
                                    "event listener is being handled",
                                );
                            }   // otherwise no service is needed
                        } else {
                            log(
                                LogType::Trace,
                                LOG_EMITTER_EVENT_REGISTRY,
                                LOG_ACTION_INSTALL,
                                Some((&name, id)),
                                LOG_WHEN_PROC,
                                LOG_STATUS_FAIL,
                                "event listener cannot be handled",
                            );
                        };
                        // also accept an error and remove the name of the
                        // event from the install queue: in the installation
                        // section the queue cannot be simply emptied
                        let i = install_queue
                            .clone()
                            .lock()
                            .expect("cannot lock event service install queue")
                            .iter()
                            .position(|x| *x == name)
                            .unwrap();
                        install_queue
                            .clone()
                            .lock()
                            .expect("cannot lock event service install queue")
                            .remove(i);
                    }
                }
                // at this point the install queue might still contain some
                // event names, that is the ones that could not be replaced
                drop(install_queue);

                // test whether any of the listening services in the cleanup
                // list has stopped, and if so remove the corresponding event
                // from the current list (sort of garbage collection)
                let ce: Vec<String> = Vec::from(cleanup_events.iter().map(|x| String::from(x)).collect::<Vec<_>>());
                for name in ce {
                    let r1 = registry.clone();
                    let id = r1.lock().unwrap().event_id(&name).unwrap();
                    if !r1.lock().unwrap().service_running_for(&name) {
                        if r1.lock().unwrap().remove_event(&name).is_ok() {
                            log(
                                LogType::Trace,
                                LOG_EMITTER_EVENT_REGISTRY,
                                LOG_ACTION_UNINSTALL,
                                Some((&name, id)),
                                LOG_WHEN_PROC,
                                LOG_STATUS_OK,
                                "event removed from registry",
                            );
                            cleanup_events.remove(
                                cleanup_events
                                .iter()
                                .position(|x| *x == name)
                                .unwrap()
                            );
                        } else {
                            log(
                                LogType::Trace,
                                LOG_EMITTER_EVENT_REGISTRY,
                                LOG_ACTION_UNINSTALL,
                                Some((&name, id)),
                                LOG_WHEN_PROC,
                                LOG_STATUS_FAIL,
                                "event could NOT be removed from registry",
                            );
                        }
                    }
                }

                if let Ok(quit) = registry
                    .clone()
                    .lock()
                    .unwrap()
                    .event_service_manager_exiting
                    .read() {
                    if *quit {
                        log(
                            LogType::Trace,
                            LOG_EMITTER_EVENT_REGISTRY,
                            LOG_ACTION_MAIN_LISTENER,
                            None,
                            LOG_WHEN_END,
                            LOG_STATUS_OK,
                            "stopping main event service manager",
                        );
                        break;
                    } else {
                        thread::sleep(rest_time);
                    }
                } else {
                    // FIXME: maybe this should break and return an error?
                    thread::sleep(rest_time);
                }
            }

            // after loop exit uninstall all installed services
            log(
                LogType::Debug,
                LOG_EMITTER_EVENT_REGISTRY,
                LOG_ACTION_MAIN_LISTENER,
                None,
                LOG_WHEN_END,
                LOG_STATUS_OK,
                "stopping active event listeners",
            );
            let r0 = registry.clone();
            let el0 = r0.lock().unwrap().event_names();
            drop(r0);
            if let Some(remaining_events) = el0 {
                for name in remaining_events {
                    let r0 = registry.clone();
                    let id = r0.lock().unwrap().event_id(&name).unwrap();
                    drop(r0);
                    let r0 = registry.clone();
                    if r0.lock().unwrap().uninstall_event_service(&name).is_ok() {
                        log(
                            LogType::Trace,
                            LOG_EMITTER_EVENT_REGISTRY,
                            LOG_ACTION_INSTALL,
                            Some((&name, id)),
                            LOG_WHEN_PROC,
                            LOG_STATUS_OK,
                            "stopped handling event listener",
                        );
                    } else {
                        log(
                            LogType::Trace,
                            LOG_EMITTER_EVENT_REGISTRY,
                            LOG_ACTION_INSTALL,
                            Some((&name, id)),
                            LOG_WHEN_PROC,
                            LOG_STATUS_FAIL,
                            "could not stop handling event listener",
                        );
                    };
                }
            }

            // join all remaining thread handles in the end
            let names: Vec<String> = Vec::from(
                service_handles
                .keys()
                .map(|x| String::from(x)).collect::<Vec<_>>()
            );
            for name in names {
                let h = service_handles.remove(&name);
                if let Some(h) = h {
                    let _ = h.join();
                }
            }
            log(
                LogType::Debug,
                LOG_EMITTER_EVENT_REGISTRY,
                LOG_ACTION_MAIN_LISTENER,
                None,
                LOG_WHEN_END,
                LOG_STATUS_OK,
                "main event service manager stopped",
            );
            Ok(true)
        });

        log(
            LogType::Debug,
            LOG_EMITTER_EVENT_REGISTRY,
            LOG_ACTION_MAIN_LISTENER,
            None,
            LOG_WHEN_START,
            LOG_STATUS_OK,
            "main event service manager start requested",
        );
        Ok(_handle)
    }

    /// Stop the event service manager thread
    pub fn stop_event_service_manager(registry: &'static Self) -> Result<(), std::io::Error> {
        log(
            LogType::Debug,
            LOG_EMITTER_EVENT_REGISTRY,
            LOG_ACTION_MAIN_LISTENER,
            None,
            LOG_WHEN_END,
            LOG_STATUS_OK,
            "main event service manager stop requested",
        );
        if let Ok(mut quit) = registry.event_service_manager_exiting.write() {
            *quit = true;
            Ok(())
        } else {
            Err(std::io::Error::new(
                ErrorKind::PermissionDenied,
                ERR_EVENTREG_CANNOT_STOP_SERVICE_MGR,
            ))
        }
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
        self.event_list
            .read()
            .expect("cannot read event registry")
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
            let el0 = self.event_list
                .read()
                .expect("cannot read event registry");
            let found_event = el0
                .get(name.as_str())
                .unwrap()
                .clone();
            drop(el0);
            let equals = found_event
                .read()
                .expect("cannot read event")
                .eq(event);
            return equals;
        }

        false
    }

    /// Check whether or not the listening service for the provided event
    /// name is running
    ///
    /// # Arguments
    ///
    /// * name - the event name
    ///
    /// # Panics
    ///
    /// May panic if the event registry could not be locked for enquiry
    /// or the contained event cannot be locked for comparison.
    pub fn service_running_for(&self, name: &str) -> bool {
        if self.has_event(name) {
            let el0 = self.event_list
                .read()
                .expect("cannot read event registry");
            let found_event = el0
                .get(name)
                .unwrap()
                .clone();
            drop(el0);
            let Ok(running) = found_event
                .read()
                .expect("cannot read event")
                .thread_running() else {
                return false
            };
            running
        } else {
            false
        }
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
    ///                   provided to the function as a `Box<dyn Event>`
    ///
    /// # Returns
    ///
    /// * `Ok(true)` - the event could be added to the registry
    /// * `Ok(false)` - the event could not be inserted
    ///
    /// **Note**: the event is _moved_ into the registry, and can only be
    ///           released (and given back stored in a `Box`) using the
    ///           `remove_event` function. Also, although the possible
    ///           outcomes include an error condition, `Err(_)` is never
    ///           returned.
    ///
    /// # Panics
    ///
    /// May panic if the event registry could not be locked for insertion.
    pub fn add_event(&self, mut boxed_event: Box<dyn Event>) -> Result<bool, std::io::Error> {
        let name = boxed_event.get_name();
        if self.has_event(&name) {
            return Ok(false);
        }
        // only consume an ID if the event is not discarded, otherwise the
        // released event would be safe to use even when not registered
        boxed_event.set_id(generate_event_id());
        self.triggerable_event_list
            .write()
            .expect("cannot write to triggerable event registry")
            .insert(name.clone(), boxed_event.triggerable());
        self.event_list
            .write()
            .expect("cannot write to event registry")
            .insert(name, Arc::new(RwLock::new(boxed_event)));
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
    /// * `Error(ErrorKind::Unsupported, _)` - the event could not be removed
    /// * `Ok(None)` - condition not found in registry
    /// * `Ok(Event)` - the removed (_pulled out_) `Event` on success
    ///
    /// # Panics
    ///
    /// May panic if the event registry could not be locked for extraction,
    /// or if an attempt is made to extract an event that is in use (FIXME:
    /// maybe it should return an error in this case?).
    pub fn remove_event(&self, name: &str) -> Result<Option<Box<dyn Event>>, std::io::Error> {
        if self.has_event(name) {
            if let Some(e) = self.event_list
                .write()
                .expect("cannot write to event registry")
                .remove(name) {
                // in this case if the event cannot be extracted from the list
                // no reference to the event is returned, but an error instead
                // WARNING: the reference is dropped in that case!
                let e = Arc::try_unwrap(e);
                let Ok(event) = e else {
                    return Err(Error::new(
                        ErrorKind::Unsupported,
                        ERR_EVENTREG_CANNOT_PULL_EVENT,
                    ));
                };
                let mut event = event
                    .into_inner()
                    .expect("cannot extract locked event");
                event.set_id(0);
                Ok(Some(event))
            } else {
                Err(Error::new(
                    ErrorKind::Unsupported,
                    ERR_EVENTREG_CANNOT_REMOVE_EVENT,
                ))
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

        for name in self.event_list
            .read()
            .expect("cannot read event registry")
            .keys() {
            res.push(name.clone())
        }
        if res.is_empty() {
            None
        } else {
            Some(res)
        }
    }

    /// Return the id of the specified event
    pub fn event_id(&self, name: &str) -> Option<i64> {
        if self.has_event(name) {
            let el0 = self.event_list
                .read()
                .expect("cannot read event registry");
            let event = el0
                .get(name)
                .expect("cannot retrieve event")
                .clone();
            drop(el0);
            let id = event.read().expect("cannot read event").get_id();
            Some(id)
        } else {
            None
        }
    }

    /// Tell whether or not an event is triggerable, `None` if event not found
    pub fn event_triggerable(&self, name: &str) -> Option<bool> {
        if self.has_event(name) {
            let triggerable = *self.triggerable_event_list
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
    pub fn trigger_event(&self, name: &str) -> std::io::Result<bool> {
        assert!(self.has_event(name), "event {name} not in registry");
        assert!(self.event_triggerable(name).unwrap(), "event {name} cannot be manually triggered");

        // what follows just *reads* the registry: the event is retrieved
        // and the corresponding structure is operated in a way that mutates
        // only its inner state, and not the wrapping pointer
        let id = self.event_id(name).unwrap();
        let el0 = self.event_list
            .read()
            .expect("cannot read event registry");
        let e0 = el0.get(name)
            .expect("cannot retrieve event for triggering")
            .clone();

        let event = e0.read()
            .expect("cannot read event for triggering");

        log(
            LogType::Trace,
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
                Ok(res)
            }
            Err(e) =>  {
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


    /// Install the listening service for an event.
    ///
    /// Return a handle to a separate thread if the service requires it,
    /// otherwise return `None`.
    ///
    /// # Panics
    ///
    /// When the event it is called upon is not registered: in no way this
    /// should be called for unregistered events.
    fn install_event_service(&self, name: &str) -> std::io::Result<Option<JoinHandle<Result<bool, Error>>>> {
        assert!(self.has_event(name), "event {name} not in registry");

        // what follows just *reads* the registry: the event is retrieved
        // and the corresponding structure is operated in a way that mutates
        // only its inner state, and not the wrapping pointer
        let id = self.event_id(name).unwrap();
        let el0 = self.event_list
            .read()
            .expect("cannot read event registry");
        let event = el0.get(name)
            .expect("cannot retrieve event for service setup")
            .clone();

        let name_copy = String::from(name);
        let event_name = Arc::new(Mutex::new(name_copy));
        let requires_thread = event
            .read()
            .expect("cannot read event for service setup")
            .requires_thread();
        if requires_thread {
            log(
                LogType::Debug,
                LOG_EMITTER_EVENT_REGISTRY,
                LOG_ACTION_INSTALL,
                Some((name, id)),
                LOG_WHEN_START,
                LOG_STATUS_OK,
                "installing event listener (dedicated thread)",
            );
            let event = event.clone();
            let event_name = String::from(event_name.clone().lock().unwrap().as_str());
            let (tx, rx) = mpsc::channel::<()>();

            // WARNING: the following is the only place where the event is
            // modified in order to listen for a potential `quit` signal:
            // it should be safe as the operation is quick in any case
            if let Ok(mut event) = event.clone().write() {
                event.assign_quit_sender(tx.clone());
            } else {
                log(
                    LogType::Warn,
                    LOG_EMITTER_EVENT_REGISTRY,
                    LOG_ACTION_INSTALL,
                    Some((name, id)),
                    LOG_WHEN_START,
                    LOG_STATUS_FAIL,
                    "cannot initialize communication with event listener",
                );
                return Err(std::io::Error::new(
                    ErrorKind::ResourceBusy,
                    "communication with event listener not estabilished",
                ));
            }

            // the actual service thread
            let handle = thread::spawn(move || {
                let name = event_name.as_str();

                // this implements the listening service in current thread
                let res = event.read().unwrap().run_service(Some(rx));
                match res {
                    Ok(ssres) => {
                        if ssres {
                            log(
                                LogType::Debug,
                                LOG_EMITTER_EVENT_REGISTRY,
                                LOG_ACTION_UNINSTALL,
                                Some((&name, id)),
                                LOG_WHEN_END,
                                LOG_STATUS_OK,
                                "event listener successfully shut down",
                            );
                            Ok(true)
                        } else {
                            log(
                                LogType::Error,
                                LOG_EMITTER_EVENT_REGISTRY,
                                LOG_ACTION_UNINSTALL,
                                Some((&name, id)),
                                LOG_WHEN_END,
                                LOG_STATUS_FAIL,
                                "event listener unsuccessfully shut down",
                            );
                            Ok(false)
                        }
                    }
                    Err(e) => {
                        log(
                            LogType::Error,
                            LOG_EMITTER_EVENT_REGISTRY,
                            LOG_ACTION_UNINSTALL,
                            Some((&name, id)),
                            LOG_WHEN_END,
                            LOG_STATUS_FAIL,
                            &format!("event listener exited with error: {e}"),
                        );
                        Err(e)
                    }
                }
            });
            Ok(Some(handle))
        } else {
            let e = event
                .read()
                .expect("cannot read event for service setup");
            if e.run_service(None)? {
                log(
                    LogType::Debug,
                    LOG_EMITTER_EVENT_REGISTRY,
                    LOG_ACTION_INSTALL,
                    Some((name, id)),
                    LOG_WHEN_START,
                    LOG_STATUS_OK,
                    "event listener installed",
                );
                Ok(None)
            } else {
                log(
                    LogType::Error,
                    LOG_EMITTER_EVENT_REGISTRY,
                    LOG_ACTION_INSTALL,
                    Some((name, id)),
                    LOG_WHEN_START,
                    LOG_STATUS_FAIL,
                    "event listener NOT installed",
                );
                Err(std::io::Error::new(
                    ErrorKind::Unsupported,
                    ERR_EVENTREG_SERVICE_NOT_INSTALLED,
                ))
            }
        }
    }


    /// Remove the installed service for an event.
    ///
    /// # Panics
    ///
    /// When the event it is called upon is not registered: in no way this
    /// should be called for unregistered events.
    fn uninstall_event_service(&self, name: &str) -> std::io::Result<()> {
        assert!(self.has_event(name), "event {name} not in registry");

        // what follows just *reads* the registry: the event is retrieved
        // and the corresponding structure is operated in a way that mutates
        // only its inner state, and not the wrapping pointer
        let id = self.event_id(name).unwrap();
        let el0 = self.event_list
            .read()
            .expect("cannot read event registry");
        let e0 = el0.get(name)
            .expect("cannot retrieve event for service removal")
            .clone();

        let event = e0
            .read()
            .expect("cannot read event for service removal");

        if event.requires_thread() {
            log(
                LogType::Debug,
                LOG_EMITTER_EVENT_REGISTRY,
                LOG_ACTION_UNINSTALL,
                Some((name, id)),
                LOG_WHEN_END,
                LOG_STATUS_OK,
                "requesting removal of event listener (dedicated thread)",
            );
            let _ = event.stop_service()?;

            Ok(())
        } else {
            if event.stop_service()? {
                log(
                    LogType::Debug,
                    LOG_EMITTER_EVENT_REGISTRY,
                    LOG_ACTION_UNINSTALL,
                    Some((name, id)),
                    LOG_WHEN_END,
                    LOG_STATUS_OK,
                    "event listener removed",
                );
                Ok(())
            } else {
                log(
                    LogType::Error,
                    LOG_EMITTER_EVENT_REGISTRY,
                    LOG_ACTION_UNINSTALL,
                    Some((name, id)),
                    LOG_WHEN_END,
                    LOG_STATUS_FAIL,
                    "event listener could NOT be removed",
                );
                Err(std::io::Error::new(
                    ErrorKind::Unsupported,
                    ERR_EVENTREG_SERVICE_NOT_UNINSTALLED,
                ))
            }
        }
    }


    /// Start listening for an event
    ///
    /// # Panics
    ///
    /// When the event it is called upon is not registered: in no way this
    /// should be called for unregistered events.
    pub fn listen_for(&self, name: &str) -> std::io::Result<()> {
        assert!(self.has_event(name), "event {name} not in registry");

        let queue = self.event_service_install_queue.clone();
        let mut locked_queue = queue
            .lock()
            .expect("cannot lock event service install queue");
        let sname = String::from(name);
        if locked_queue.contains(&sname) {
            let index = locked_queue.iter().position(|s| *s == sname).unwrap();
            locked_queue.remove(index);
        }
        locked_queue.push(sname);
        let id = self.event_id(name).unwrap();
        log(
            LogType::Debug,
            LOG_EMITTER_EVENT_REGISTRY,
            LOG_ACTION_INSTALL,
            Some((name, id)),
            LOG_WHEN_START,
            LOG_STATUS_OK,
            "event listener installation requested",
        );

        Ok(())
    }


    /// Stop listening for an event
    ///
    /// # Panics
    ///
    /// When the event it is called upon is not registered: in no way this
    /// should be called for unregistered events.
    pub fn unlisten_for(&self, name: &str) -> std::io::Result<()> {
        assert!(self.has_event(name), "event {name} not in registry");

        let sname = String::from(name);
        let queue = self.event_service_uninstall_queue.clone();
        let mut locked_queue = queue
            .lock()
            .expect("cannot lock event service uninstall queue");
        if !locked_queue.contains(&sname) {
            locked_queue.push(sname);
        }
        let id = self.event_id(name).unwrap();
        log(
            LogType::Debug,
            LOG_EMITTER_EVENT_REGISTRY,
            LOG_ACTION_UNINSTALL,
            Some((name, id)),
            LOG_WHEN_END,
            LOG_STATUS_OK,
            "event listener removal requested",
        );

        Ok(())
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
    pub fn fire_condition_for(&self, name: &str) {
        assert!(self.has_event(name), "event {name} not in registry");

        // what follows just *reads* the registry: the event is retrieved
        // and the corresponding structure is operated in a way that mutates
        // only its inner state, and not the wrapping pointer
        let id = self.event_id(name).unwrap();
        let el0 = self.event_list
            .read()
            .expect("cannot read event registry");
        let e0 = el0.get(name)
            .expect("cannot retrieve event for activation")
            .clone();

        let event = e0.read()
            .expect("cannot read event for activation");
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
