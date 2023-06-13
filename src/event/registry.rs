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
use std::collections::HashMap;
use std::io::{Error, ErrorKind};
use std::thread;

use lazy_static::lazy_static;
use unique_id::Generator;
use unique_id::sequence::SequenceGenerator;

use super::base::Event;
use crate::common::logging::{log, LogType};


// module-wide values
lazy_static! {
    // the main task ID generator
    static ref UID_GENERATOR: SequenceGenerator = {
        let mut _uidgen = SequenceGenerator;
        _uidgen
    };
}

// the specific task ID generator: used internally to register an event
#[allow(dead_code)]
fn generate_event_id() -> i64 {
    UID_GENERATOR.next_id()
}



/// The event registry: there must be one and only one event registry in each
/// instance of the process, and should have `'static` lifetime. It may be
/// passed around as a reference for events.
pub struct EventRegistry {
    // the entire list is enclosed in `Arc<Mutex<...>>` in order to avoid
    // concurrent access to the list itself
    event_list: Arc<Mutex<HashMap<String, Arc<Mutex<Box<dyn Event>>>>>>,
}


#[allow(dead_code)]
impl EventRegistry {

    /// Create a new, empty `EventRegistry`.
    pub fn new() -> Self {
        EventRegistry {
            event_list: Arc::new(Mutex::new(HashMap::new())),
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
            .lock()
            .expect("cannot lock event registry")
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
        self.event_list
            .lock()
            .expect("cannot lock event registry")
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
    /// * `Error(Errorkind::Unsupported, _)` - the event could not be removed
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
                .lock()
                .expect("cannot lock event registry")
                .remove(name) {
                let Ok(mx) = Arc::try_unwrap(r) else {
                    panic!("attempt to extract referenced event {name}")
                };
                let mut event = mx
                    .into_inner()
                    .expect(&format!("attempt to extract locked event {name}"));    // <- may have to fix this
                event.set_id(0);
                Ok(Some(event))
            } else {
                Err(Error::new(
                    ErrorKind::Unsupported,
                    format!("cannot pull condition {name} back from registry")))
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
            .lock()
            .expect("cannot lock event registry")
            .keys()
            .into_iter() {
            res.push(name.clone())
        }
        if res.is_empty() {
            None
        } else {
            Some(res)
        }
    }


    /// Install the listening service for an event.
    ///
    /// Return a handle to a separate thread if the service requires it,
    /// otherwise it returns `None`.
    ///
    /// # Panics
    ///
    /// When the event it is called upon is not registered: in no way this
    /// should be called for unregistered events.
    pub fn install_service(&self, name: &str) -> std::io::Result<Option<thread::JoinHandle<Result<bool, Error>>>> {
        if !self.has_event(name) {
            panic!("event {name} not found in registry");
        }

        let event;
        {
            let elist = self.event_list.clone();
            let guard = elist.lock().expect("cannot lock event registry");
            event = guard.get(name)
                .expect(&format!("cannot retrieve event {name} for activation"))
                .clone()
        }

        let mxevent = event.lock().expect(&format!("cannot lock event {name} for activation"));
        let name_copy = String::from(name);
        let event_name = Arc::new(Mutex::new(name_copy));
        if mxevent.requires_thread() {
            log(
                LogType::Debug,
                "EVENT_REGISTRY install",
                &format!("[START/OK] installing listening service for event {name} (dedicated thread)"),
            );
            let event = event.clone();
            let event_name = event_name.clone();
            let handle = thread::spawn(move || {
                let ename = event_name.lock().unwrap();
                let res = event.lock().unwrap()._start_service();
                match res {
                    Ok(ssres) => {
                        if ssres {
                            log(
                                LogType::Debug,
                                "EVENT_REGISTRY install",
                                &format!("[START/OK] listening service installed for event {ename}"),
                            );
                        } else {
                            log(
                                LogType::Error,
                                "EVENT_REGISTRY install",
                                &format!("[START/FAIL] listening service for event {ename} NOT installed"),
                            );
                        }
                        Ok(ssres)
                    }
                    Err(e) => {
                        log(
                            LogType::Error,
                            "EVENT_REGISTRY install",
                            &format!(
                                "[START/FAIL] listening service for event {ename} NOT installed: {}",
                                e.to_string(),
                            ),
                        );
                        Err(e)
                    }
                }
            });
            Ok(Some(handle))
        } else {
            if mxevent._start_service()? {
                log(
                    LogType::Debug,
                    "EVENT_REGISTRY install",
                    &format!("[START/OK] installing listening service for event {name}"),
                );
            } else {
                // FIXME: this might have to return Err(...) instead!?
                log(
                    LogType::Error,
                    "EVENT_REGISTRY install",
                    &format!("[START/FAIL] listening service for event {name} NOT installed"),
                );
            }
            Ok(None)
        }
    }


    /// Fire the condition associated to the named event.
    ///
    /// This version calls in turn the events `fire_condition()` method, but
    /// has the advantage of being implemented on an object that is has a
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

        let event;
        {
            let elist = self.event_list.clone();
            let guard = elist.lock().expect("cannot lock event registry");
            event = guard.get(name)
                .expect(&format!("cannot retrieve event {name} for activation"))
                .clone()
        }

        let mxevent = event.lock()
            .expect(&format!("cannot lock event {name} for activation"));
        if let Ok(res) = mxevent.fire_condition() {
            if res {
                log(
                    LogType::Trace,
                    "EVENT_REGISTRY fire",
                    &format!("[PROC/OK] condition for event {name} fired"),
                );
            } else {
                log(
                    LogType::Trace,
                    "EVENT_REGISTRY fire",
                    &format!("[PROC/FAIL] condition for event {name} could not fire"),
                );
            }
        } else {
            log(
                LogType::Debug,
                "EVENT_REGISTRY fire",
                &format!("[PROC/FAIL] condition for event {name} failed to fire"),
            );
        }
    }

}


// end.