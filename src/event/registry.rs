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
    event_list: RwLock<HashMap<String, Arc<Mutex<Box<dyn Event>>>>>,
    // the triggerable list is kept separate because the triggerable
    // attribute is actually a constant that can be retrieved at startup
    // and we do not want to be blocked while directly asking an active
    // event on its ability to be manually triggered
    triggerable_event_list: RwLock<HashMap<String, bool>>,
    // the queue of events whose services need to be installed
    event_service_install_queue: Arc<Mutex<Vec<String>>>,
    // the queue of events whose services are up to be removed
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
    pub fn start_event_service_manager(registry: &'static Self) -> Result<JoinHandle<Result<bool, std::io::Error>>, std::io::Error> {
        // self can be expected to be &'static mut because we know that this
        // registry lives as much as the entire program instance lives
        let rest_time = Duration::from_millis(MAIN_EVENT_REGISTRY_MGMT_MILLISECONDS);
        
        let registry = Arc::new(Mutex::new(Box::new(registry)));

        let install_queue = registry.clone().lock().unwrap().event_service_install_queue.clone();
        let uninstall_queue = registry.clone().lock().unwrap().event_service_uninstall_queue.clone();
        let registry_ref = registry.clone();
        let _handle = thread::spawn(move || {
            log(
                LogType::Debug,
                LOG_EMITTER_EVENT_REGISTRY,
                LOG_ACTION_MAIN_LISTENER,
                None,
                LOG_WHEN_START,
                LOG_STATUS_OK,
                &format!("starting main event service manager"),
            );
            loop {
                let names: Vec<String>;
                {
                    names = uninstall_queue
                        .lock()
                        .expect("cannot lock event service uninstall queue")
                        .clone();
                }
                for name in names {
                    let t= registry_ref.clone();
                    if let Ok(_) = t.lock().unwrap().uninstall_event_service(&name) {
                        let id = registry_ref.lock().unwrap().event_id(&name).unwrap();
                        log(
                            LogType::Debug,
                            LOG_EMITTER_EVENT_REGISTRY,
                            LOG_ACTION_INSTALL,
                            Some((&name, id)),
                            LOG_WHEN_PROC,
                            LOG_STATUS_OK,
                            &format!("stopped handling listening service for event {name}"),
                        );
                    } else {
                        let id = registry_ref.lock().unwrap().event_id(&name).unwrap();
                        log(
                            LogType::Debug,
                            LOG_EMITTER_EVENT_REGISTRY,
                            LOG_ACTION_INSTALL,
                            Some((&name, id)),
                            LOG_WHEN_PROC,
                            LOG_STATUS_FAIL,
                            &format!("could not stop handling listening service for event {name}"),
                        );
                    };
                }
                uninstall_queue.clone().lock().expect("cannot lock event service uninstall queue").clear();
                let names: Vec<String>;
                {
                    names = install_queue
                        .lock()
                        .expect("cannot lock event service install queue")
                        .clone();
                }
                for name in names {
                    let t= registry_ref.clone();
                    if let Ok(o) = t.lock().unwrap().install_event_service(&name) {
                        if let Some(service) = o {
                            let id = registry_ref.lock().unwrap().event_id(&name).unwrap();
                            if service.join().is_ok() {
                                log(
                                    LogType::Debug,
                                    LOG_EMITTER_EVENT_REGISTRY,
                                    LOG_ACTION_INSTALL,
                                    Some((&name, id)),
                                    LOG_WHEN_PROC,
                                    LOG_STATUS_OK,
                                    &format!("listening service for event {name} is being handled"),
                                );
                            } else {
                                log(
                                    LogType::Debug,
                                    LOG_EMITTER_EVENT_REGISTRY,
                                    LOG_ACTION_INSTALL,
                                    Some((&name, id)),
                                    LOG_WHEN_PROC,
                                    LOG_STATUS_FAIL,
                                    &format!("listening service for event {name} NOT handled"),
                                );
                            }
                        }
                    } else {
                        let id = registry_ref.lock().unwrap().event_id(&name).unwrap();
                        log(
                            LogType::Debug,
                            LOG_EMITTER_EVENT_REGISTRY,
                            LOG_ACTION_INSTALL,
                            Some((&name, id)),
                            LOG_WHEN_PROC,
                            LOG_STATUS_FAIL,
                            &format!("listening service for event {name} cannot be handled"),
                        );
                    };
                }
                install_queue.lock().expect("cannot lock event service install queue").clear();
                if let Ok(quit) = registry_ref.lock().unwrap().event_service_manager_exiting.read() {
                    if *quit {
                        log(
                            LogType::Debug,
                            LOG_EMITTER_EVENT_REGISTRY,
                            LOG_ACTION_MAIN_LISTENER,
                            None,
                            LOG_WHEN_END,
                            LOG_STATUS_OK,
                            &format!("stopping main event service manager"),
                        );
                        break;
                    } else {
                        thread::sleep(rest_time);
                    }
                } else {
                    // FIXME: this should break and return an error?
                    thread::sleep(rest_time);
                }
            }
            // after loop exit uninstall all installed services
            if let Some(remainig_events) = registry_ref.lock().unwrap().event_names() {
                for name in remainig_events {
                    if let Ok(_) = registry_ref.lock().unwrap().uninstall_event_service(name.as_str()) {
                        let id = registry_ref.lock().unwrap().event_id(name.as_str()).unwrap();
                        log(
                            LogType::Debug,
                            LOG_EMITTER_EVENT_REGISTRY,
                            LOG_ACTION_INSTALL,
                            Some((name.as_str(), id)),
                            LOG_WHEN_PROC,
                            LOG_STATUS_OK,
                            &format!("stopped handling listening service for event {name}"),
                        );
                    } else {
                        let id = registry_ref.lock().unwrap().event_id(name.as_str()).unwrap();
                        log(
                            LogType::Debug,
                            LOG_EMITTER_EVENT_REGISTRY,
                            LOG_ACTION_INSTALL,
                            Some((name.as_str(), id)),
                            LOG_WHEN_PROC,
                            LOG_STATUS_FAIL,
                            &format!("could not stop handling listening service for event {name}"),
                        );
                    };
                }
            }
            Ok(true)
        });
        Ok(_handle)
    }

    /// Stop the event service manager thread
    pub fn stop_event_service_manager(registry: &'static Self) -> Result<(), std::io::Error> {
        if let Ok(mut quit) = registry.event_service_manager_exiting.write() {
            *quit = true;
            Ok(())
        } else {
            Err(std::io::Error::new(
                ErrorKind::PermissionDenied,
                "could not request the event service manager to shut down",
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
        let triggerable = boxed_event.triggerable();
        self.triggerable_event_list
            .write()
            .expect("cannot write to triggerable event registry")
            .insert(name.clone(), triggerable);
        self.event_list
            .write()
            .expect("cannot write to event registry")
            .insert(name, Arc::new(Mutex::new(boxed_event)));
        Ok(true)
    }

    /// Remove a named event from the list and give it back stored in a Box.
    ///
    /// The returned `Event` can be modified and stored back in the
    /// registry: before returning, the boxed `Event` is 'uninitialized'
    /// (that is, its ID is set back to 0) so that it wouldn't be checked if
    /// asked to; the rest of its internal status is preserved
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
            if let Some(r) = self.event_list
                .write()
                .expect("cannot write to event registry")
                .remove(name) {
                let Ok(mx) = Arc::try_unwrap(r) else {
                    panic!("cannot extract referenced event {name}")
                };
                // WARNING: we should also uninstall the related service if any!
                let mut event = mx
                    .into_inner()
                    .expect("cannot extract locked event");
                event.set_id(0);
                Ok(Some(event))
            } else {
                Err(Error::new(
                    ErrorKind::Unsupported,
                    ERR_EVENTREG_CANNOT_PULL_EVENT,
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
        let guard;
        if self.has_event(name) {
            guard = self.event_list
                .read()
                .expect("cannot read event registry");
        } else {
            return None
        }
        let event = guard
            .get(name)
            .expect("cannot retrieve event")
            .clone();
        drop(guard);
        let id = event.lock().expect("cannot lock event").get_id();
        Some(id)
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
        if !self.has_event(name) {
            panic!("event {name} not found in registry");
        }

        // also panic if the event is not triggerable: the caller must ensure this
        if !self.event_triggerable(name).unwrap() {
            panic!("event {name} cannot be manually triggered");
        }

        // what follows just *reads* the registry: the event is retrieved
        // and the corresponding structure is operated in a way that mutates
        // only its inner state, and not the wrapping pointer
        let id = self.event_id(name).unwrap();
        let guard = self.event_list
            .read()
            .expect("cannot read event registry");
        let event = guard.get(name)
            .expect("cannot retrieve event for triggering")
            .clone();

        let mxevent = event.lock()
            .expect("cannot lock event for triggering");

        log(
            LogType::Trace,
            LOG_EMITTER_EVENT_REGISTRY,
            LOG_ACTION_TRIGGER,
            Some((name, id)),
            LOG_WHEN_PROC,
            LOG_STATUS_OK,
            &format!("manually triggering event {name}"),
        );
        match mxevent.fire_condition() {
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
        if !self.has_event(name) {
            panic!("event {name} not found in registry");
        }

        // what follows just *reads* the registry: the event is retrieved
        // and the corresponding structure is operated in a way that mutates
        // only its inner state, and not the wrapping pointer
        let id = self.event_id(name).unwrap();
        let guard = self.event_list
            .read()
            .expect("cannot read event registry");
        let event = guard.get(name)
            .expect("cannot retrieve event for service setup")
            .clone();

        let mxevent = event.lock().expect("cannot lock event for service setup");
        let name_copy = String::from(name);
        let event_name = Arc::new(Mutex::new(name_copy));
        if mxevent.requires_thread() {
            log(
                LogType::Debug,
                LOG_EMITTER_EVENT_REGISTRY,
                LOG_ACTION_INSTALL,
                Some((name.as_ref(), id)),
                LOG_WHEN_START,
                LOG_STATUS_OK,
                &format!("installing listening service for event {name} (dedicated thread)"),
            );
            let event = event.clone();
            let event_name = event_name.clone();
            let handle = thread::spawn(move || {
                let ename = event_name.lock().unwrap();

                // this implements the listening service in current thread
                let res = event.lock().unwrap()._run_service();
                match res {
                    Ok(ssres) => {
                        if ssres {
                            log(
                                LogType::Debug,
                                LOG_EMITTER_EVENT_REGISTRY,
                                LOG_ACTION_INSTALL,
                                Some((&ename, id)),
                                LOG_WHEN_START,
                                LOG_STATUS_OK,
                                &format!("listening service for event {ename} exited successfully"),
                            );
                            Ok(true)
                        } else {
                            log(
                                LogType::Error,
                                LOG_EMITTER_EVENT_REGISTRY,
                                LOG_ACTION_INSTALL,
                                Some((&ename, id)),
                                LOG_WHEN_START,
                                LOG_STATUS_FAIL,
                                &format!("listening service for event {ename} exited unsuccessfully"),
                            );
                            Ok(false)
                        }
                    }
                    Err(e) => {
                        log(
                            LogType::Error,
                            LOG_EMITTER_EVENT_REGISTRY,
                            LOG_ACTION_INSTALL,
                                Some((&ename, id)),
                            LOG_WHEN_START,
                            LOG_STATUS_FAIL,
                            &format!("listening service for event {ename} exited with error: {e}"),
                        );
                        Err(e)
                    }
                }
            });
            Ok(Some(handle))
        } else {
            if mxevent._run_service()? {
                log(
                    LogType::Debug,
                    LOG_EMITTER_EVENT_REGISTRY,
                    LOG_ACTION_INSTALL,
                    Some((name, id)),
                    LOG_WHEN_START,
                    LOG_STATUS_OK,
                    &format!("listening service for event {name} installed"),
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
                    &format!("listening service for event {name} NOT installed"),
                );
                Err(std::io::Error::new(
                    ErrorKind::Unsupported,
                    format!("listening service for event {name} NOT installed"),
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
        if !self.has_event(name) {
            panic!("event {name} not found in registry");
        }

        // what follows just *reads* the registry: the event is retrieved
        // and the corresponding structure is operated in a way that mutates
        // only its inner state, and not the wrapping pointer
        let id = self.event_id(name).unwrap();
        let guard = self.event_list
            .read()
            .expect("cannot read event registry");
        let event = guard.get(name)
            .expect("cannot retrieve event for service setup")
            .clone();

        let mxevent = event.lock().expect("cannot lock event for service setup");

        if mxevent.requires_thread() {
            log(
                LogType::Debug,
                LOG_EMITTER_EVENT_REGISTRY,
                LOG_ACTION_UNINSTALL,
                Some((name.as_ref(), id)),
                LOG_WHEN_END,
                LOG_STATUS_OK,
                &format!("requesting removal of listening service for event {name} (dedicated thread)"),
            );
            let _ = mxevent._stop_service()?;

            Ok(())
        } else {
            if mxevent._stop_service()? {
                log(
                    LogType::Debug,
                    LOG_EMITTER_EVENT_REGISTRY,
                    LOG_ACTION_UNINSTALL,
                    Some((name, id)),
                    LOG_WHEN_END,
                    LOG_STATUS_OK,
                    &format!("listening service for event {name} removed"),
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
                    &format!("listening service for event {name} could NOT be removed"),
                );
                Err(std::io::Error::new(
                    ErrorKind::Unsupported,
                    format!("listening service for event {name} could NOT be removed"),
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
        if !self.has_event(name) {
            panic!("event {name} not found in registry");
        }

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
            &format!("service installation for event {name} requested"),
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
        if !self.has_event(name) {
            panic!("event {name} not found in registry");
        }
        let queue = self.event_service_uninstall_queue.clone();
        let mut locked_queue = queue
            .lock()
            .expect("cannot lock event service uninstall queue");
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
            &format!("service removal for event {name} requested"),
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
        if !self.has_event(name) {
            panic!("event {name} not found in registry");
        }

        // what follows just *reads* the registry: the event is retrieved
        // and the corresponding structure is operated in a way that mutates
        // only its inner state, and not the wrapping pointer
        let id = self.event_id(name).unwrap();
        let guard = self.event_list
            .read()
            .expect("cannot read event registry");
        let event = guard.get(name)
            .expect("cannot retrieve event for activation")
            .clone();

        let mxevent = event.lock()
            .expect("cannot lock event for activation");
        if let Ok(res) = mxevent.fire_condition() {
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
