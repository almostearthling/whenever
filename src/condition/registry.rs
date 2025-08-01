//! # Condition Registry
//!
//! `condition::registry` implements the main registry for `Condition` objects.
//!
//! Implements the condition registry as the main interface to access and check
//! _active_ conditions: a `Condition` object cannot in fact be considered
//! active until it is _registered_. A registered condition has an unique
//! nonzero ID.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::RwLock;

use lazy_static::lazy_static;
use unique_id::Generator;
use unique_id::sequence::SequenceGenerator;

use super::base::{Condition, ConditionRef};
use crate::common::logging::{LogType, log};
use crate::common::wres::{Error, Kind, Result};
use crate::constants::*;

// module-wide values
lazy_static! {
    // the main condition ID generator
    static ref UID_GENERATOR: SequenceGenerator = {

        SequenceGenerator
    };
}

// the specific condition ID generator: used internally to register a condition
#[allow(dead_code)]
fn generate_condition_id() -> i64 {
    UID_GENERATOR.next_id()
}

/// The condition registry: there must be one and only one condition registry
/// in each instance of the process, and should have `'static` lifetime. It may
/// be passed around as a reference.
pub struct ConditionRegistry {
    // the entire list is enclosed in `RwLock<...>` in order to avoid
    // concurrent access to the list itself; on the other hand, the _busy_
    // flag is kept in a `Mutex` because it changes quite dynamically
    condition_list: RwLock<HashMap<String, Arc<Mutex<ConditionRef>>>>,
    conditions_busy: Arc<Mutex<u64>>,

    // a list of conditions to be reset as soon as they become not busy
    conditions_to_reset: Arc<Mutex<Vec<String>>>,

    // a list of conditions to be suspended as soon as they become not busy
    conditions_to_suspend: Arc<Mutex<Vec<String>>>,

    // the two queues for items to remove and items to add: the items that
    // need to be added are stored as full (dyn) items, while the ones to
    // be removed are stored as names
    items_to_remove: Arc<Mutex<Vec<String>>>,
    items_to_add: Arc<Mutex<Vec<ConditionRef>>>,
}

#[allow(dead_code)]
impl ConditionRegistry {
    /// Create a new, empty `ConditionRegistry`.
    pub fn new() -> Self {
        ConditionRegistry {
            condition_list: RwLock::new(HashMap::new()),
            conditions_busy: Arc::new(Mutex::new(0_u64)),
            conditions_to_reset: Arc::new(Mutex::new(Vec::new())),
            conditions_to_suspend: Arc::new(Mutex::new(Vec::new())),

            items_to_remove: Arc::new(Mutex::new(Vec::new())),
            items_to_add: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Check whether or not a condition with the provided name is in the
    /// registry.
    ///
    /// # Arguments
    ///
    /// * name - the name of the condition to check for registration
    ///
    /// # Panics
    ///
    /// May panic if the condition registry could not be locked for enquiry.
    pub fn has_condition(&self, name: &str) -> bool {
        self.condition_list
            .read()
            .expect("cannot read condition registry")
            .contains_key(name)
    }

    /// Check whether or not a condition is in the registry.
    ///
    /// # Arguments
    ///
    /// * cond - the reference to a condition to check for registration
    ///
    /// # Panics
    ///
    /// May panic if the condition registry could not be locked for enquiry
    /// or the contained condition cannot be locked for comparison.
    pub fn has_condition_eq(&self, cond: &dyn Condition) -> bool {
        let name = cond.get_name();
        if self.has_condition(name.as_str()) {
            let conditions = self
                .condition_list
                .read()
                .expect("cannot read event registry");
            let found_condition = conditions.get(name.as_str()).unwrap();
            let c0 = found_condition.clone();
            let locked_condition = c0.lock().expect("cannot check event for comparison");
            return locked_condition.eq(cond);
        }

        false
    }

    /// Return the type of a condition given its name, or `None` if the
    /// condition is not found in the registry.
    ///
    /// # Arguments
    ///
    /// * name - the name of the condition
    ///
    /// # Panics
    ///
    /// May panic if the condition registry could not be locked for enquiry.
    pub fn condition_type(&self, name: &str) -> Option<String> {
        if self.has_condition(name) {
            self.condition_list
                .read()
                .expect("cannot read condition registry")
                .get(name)
                .map(|r| {
                    r.clone()
                        .lock()
                        .expect("cannot lock condition to retrieve type")
                        .get_type()
                        .to_string()
                })
        } else {
            None
        }
    }

    /// Add an already-boxed `Condition` if its name is not present in the
    /// registry.
    ///
    /// The `Box` ensures that the enclosed condition is transferred as a
    /// reference and stored as-is in the registry. Note that for the
    /// registration to be successful there must **not** already be a condition
    /// with the same name in the registry: if such condition is found
    /// `Ok(false)` is returned. In order to replace a `Condition` it has to be
    /// removed first, then added.
    ///
    /// # Arguments
    ///
    /// * `boxed_condition` - an object implementing the `base::Condition`
    ///   trait, provided to the function as a `Box<dyn Condition>` aka
    ///   `ConditionRef`
    ///
    /// # Returns
    ///
    /// * `Ok(true)` - the condition could be added to the registry
    /// * `Ok(false)` - the condition could not be inserted
    ///
    /// **Note**: the condition is _moved_ into the registry, and can only be
    /// released (and given back stored in a `Box`) using the `remove_condition`
    /// function. Also, although the possible outcomes include an error
    /// condition, `Err(_)` is never returned.
    ///
    /// # Panics
    ///
    /// May panic if the condition registry could not be locked for insertion.
    pub fn add_condition(&self, mut boxed_condition: ConditionRef) -> Result<bool> {
        let name = boxed_condition.get_name();
        if self.has_condition(&name) {
            return Ok(false);
        }
        // only consume an ID if the condition is not discarded, otherwise the
        // released condition would be safe to run even when not registered
        boxed_condition.set_id(generate_condition_id());
        self.condition_list
            .write()
            .expect("cannot write to condition registry")
            .insert(name, Arc::new(Mutex::new(boxed_condition)));
        Ok(true)
    }

    /// Add or replace an already-boxed `Condition` while running: if the
    /// registry is busy running any condition all modifications are deferred
    pub fn dynamic_add_or_replace_condition(&self, boxed_condition: ConditionRef) -> Result<bool> {
        let name = boxed_condition.get_name();
        let busy = self.conditions_busy.clone();
        let busy = busy.lock().expect("cannot acquire busy conditions counter");
        if *busy == 0 {
            if self.has_condition(&name) {
                match self.remove_condition(&name) {
                    Ok(_) => {
                        if let Ok(res) = self.add_condition(boxed_condition) {
                            return Ok(res);
                        } else {
                            return Err(Error::new(Kind::Failed, ERR_CONDREG_COND_NOT_REPLACED));
                        }
                    }
                    _ => {
                        return Err(Error::new(Kind::Failed, ERR_CONDREG_CANNOT_PULL_COND));
                    }
                }
            } else if let Ok(res) = self.add_condition(boxed_condition) {
                return Ok(res);
            } else {
                return Err(Error::new(Kind::Failed, ERR_CONDREG_COND_NOT_ADDED));
            }
        } else {
            let queue = self.items_to_add.clone();
            let mut queue = queue.lock().expect("cannot acquire list of items to add");
            queue.push(boxed_condition);
            log(
                LogType::Debug,
                LOG_EMITTER_TASK_REGISTRY,
                LOG_ACTION_NEW,
                None,
                LOG_WHEN_PROC,
                LOG_STATUS_OK,
                &format!(
                    "registry busy: condition {name} set to be added when no conditions are busy",
                ),
            );
        }

        Ok(true)
    }

    /// Remove a named condition from the list and give it back stored in a Box.
    ///
    /// The returned `Condition` can be modified and stored back in the
    /// registry: before returning, the boxed `Condition` is 'uninitialized'
    /// (that is, its ID is set back to 0) so that it wouldn't be checked if
    /// asked to; the rest of its internal status is preserved (*FIXME*: how
    /// can it be retrieved?)
    ///
    /// # Arguments
    ///
    /// * `name` - the name of the condition that must be removed
    ///
    /// # Returns
    ///
    /// * `Error(Kind::Failed, _)` - the condition could not be removed
    /// * `Ok(None)` - condition not found in registry
    /// * `Ok(Condition)` - the removed (_pulled out_) `Condition` on success
    ///
    /// # Panics
    ///
    /// May panic if the condition registry could not be locked for extraction,
    /// or if an attempt is made to extract a condition that is in use (FIXME:
    /// maybe it should return an error in this case? for now the scheduler is
    /// suspended while reconfiguring, the only actual case for removal).
    pub fn remove_condition(&self, name: &str) -> Result<Option<ConditionRef>> {
        if self.has_condition(name) {
            let mut cl0 = self
                .condition_list
                .write()
                .expect("cannot write to condition registry");
            match cl0.remove(name) {
                Some(c0) => {
                    drop(cl0);
                    let Ok(mxc0) = Arc::try_unwrap(c0) else {
                        panic!("cannot extract referenced condition {name}")
                    };

                    let mut condition = mxc0.into_inner().expect("cannot extract locked condition"); // <- may have to fix this
                    condition.set_id(0);
                    Ok(Some(condition))
                }
                _ => Err(Error::new(Kind::Failed, ERR_CONDREG_CANNOT_PULL_COND)),
            }
        } else {
            Ok(None)
        }
    }

    /// Remove a named condition from the list operating on a running
    /// registry: if any conditions are busy all modifications to the
    /// registry are deferred
    pub fn dynamic_remove_condition(&self, name: &str) -> Result<bool> {
        if self.has_condition(name) {
            let busy = self.conditions_busy.clone();
            let busy = busy.lock().expect("cannot acquire busy conditions counter");
            if *busy == 0 {
                self.remove_condition(name)?;
            } else {
                let queue = self.items_to_remove.clone();
                let mut queue = queue
                    .lock()
                    .expect("cannot acquire list of items to remove");
                queue.push(String::from(name));
                log(
                    LogType::Debug,
                    LOG_EMITTER_TASK_REGISTRY,
                    LOG_ACTION_UNINSTALL,
                    None,
                    LOG_WHEN_PROC,
                    LOG_STATUS_OK,
                    &format!(
                        "registry busy: condition {name} set to be removed when no conditions are busy",
                    ),
                );
            }
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Reset the named condition if found in the registry
    ///
    /// # Arguments
    ///
    /// * `name` - the name of the condition that must be reset
    /// * `wait` - if false an attempt to reset while busy returns an error
    ///
    /// # Returns
    ///
    /// * `Ok(true)` - operation succeeded, otherwise an error
    ///
    /// # Panics
    ///
    /// This function panics when called upon a name that does not exist in
    /// the registry
    pub fn reset_condition(&self, name: &str, wait: bool) -> Result<bool> {
        assert!(self.has_condition(name), "condition {name} not in registry");

        if !wait && !self.condition_is_free(name) {
            Err(Error::new(Kind::Busy, ERR_CONDREG_COND_RESET_BUSY))
        } else {
            // what follows just *reads* the registry: the condition is retrieved
            // and the corresponding structure is operated in a way that mutates
            // only its inner state, and not the wrapping pointer
            let cl0 = self
                .condition_list
                .write()
                .expect("cannot read condition registry");
            let cond = cl0.get(name).expect("cannot retrieve condition for reset");
            let cond = cond.clone();
            drop(cl0);

            // when we acquire the lock, we can safely reset the condition right
            // here and return the operation result from the condition itself
            let mut cond = cond.lock().expect("condition cannot be locked");
            cond.reset()
        }
    }

    /// Queue a condition for reset: the current policy is that the
    /// reset status will be set only when there are no busy condiions
    pub fn queue_reset_condition(&self, name: &str) -> Result<()> {
        assert!(self.has_condition(name), "condition {name} not in registry");

        let mxq0 = self.conditions_to_reset.clone();
        let mut queue = mxq0
            .lock()
            .expect("cannot lock list of conditions to reset");
        let s = String::from(name);
        if !queue.contains(&s) {
            queue.push(s);
        }

        Ok(())
    }

    /// Suspend the named condition if found in the registry
    ///
    /// # Arguments
    ///
    /// * `name` - the name of the condition that must be suspended
    /// * `wait` - if false an attempt to suspend while busy returns an error
    ///
    /// # Returns
    ///
    /// * `Ok(true)` - operation succeeded
    /// * `Ok(false)` - no state change
    ///
    /// otherwise returns an error.
    ///
    /// # Panics
    ///
    /// This function panics when called upon a name that does not exist in
    /// the registry
    pub fn suspend_condition(&self, name: &str, wait: bool) -> Result<bool> {
        assert!(self.has_condition(name), "condition {name} not in registry");

        if !wait && !self.condition_is_free(name) {
            Err(Error::new(Kind::Busy, ERR_CONDREG_COND_SUSPEND_BUSY))
        } else {
            // what follows just *reads* the registry: the condition is retrieved
            // and the corresponding structure is operated in a way that mutates
            // only its inner state, and not the wrapping pointer
            let cl0 = self
                .condition_list
                .read()
                .expect("cannot read condition registry");
            let cond = cl0
                .get(name)
                .expect("cannot retrieve condition for suspend");
            let cond = cond.clone();
            drop(cl0);

            // when we acquire the lock, we can safely reset the condition right
            // here and return the operation result from the condition itself
            let mut cond = cond.lock().expect("condition cannot be locked");
            cond.suspend()
        }
    }

    /// Queue a condition for suspension: the current policy is that the
    /// suspended flag will be set only when there are no busy condiions
    pub fn queue_suspend_condition(&self, name: &str) -> Result<()> {
        assert!(self.has_condition(name), "condition {name} not in registry");

        let mxq0 = self.conditions_to_suspend.clone();
        let mut queue = mxq0
            .lock()
            .expect("cannot lock list of conditions to suspend");
        let s = String::from(name);
        if !queue.contains(&s) {
            queue.push(s);
        }

        Ok(())
    }

    /// Resume the named condition if found in the registry
    ///
    /// # Arguments
    ///
    /// * `name` - the name of the condition that must be resumed
    /// * `wait` - if false an attempt to resume while busy returns an error
    ///
    /// # Returns
    ///
    /// * `Ok(true)` - operation succeeded
    /// * `Ok(false)` - no state change
    ///
    /// otherwise returns an error.
    ///
    /// # Panics
    ///
    /// This function panics when called upon a name that does not exist in
    /// the registry
    pub fn resume_condition(&self, name: &str, wait: bool) -> Result<bool> {
        assert!(self.has_condition(name), "condition {name} not in registry");

        // actually, a suspended condition **cannot** be busy, so the _wait_
        // parameter should not even be implemented here; however, since the
        // caller might try to invoke the operation on a condition that is
        // not suspended, before attempting to modify its state it is still
        // safer to return this error on busy conditions: another way to
        // handle this situation would be to return _Ok(false)_ here, because
        // a busy condition is certainly not suspended
        if !wait && !self.condition_is_free(name) {
            Err(Error::new(Kind::Busy, ERR_CONDREG_COND_RESUME_BUSY))
        } else {
            // what follows just *reads* the registry: the condition is retrieved
            // and the corresponding structure is operated in a way that mutates
            // only its inner state, and not the wrapping pointer
            let cl0 = self
                .condition_list
                .read()
                .expect("cannot read condition registry");
            let cond = cl0.get(name).expect("cannot retrieve condition for resume");
            let cond = cond.clone();
            drop(cl0);

            // when we acquire the lock, we can safely reset the condition right
            // here and return the operation result from the condition itself
            let mut cond = cond.lock().expect("condition cannot be locked");
            cond.resume()
        }
    }

    /// Return the list of condition names as owned strings.
    ///
    /// Return a vector containing the names of all the conditions that have
    /// been registered, as `String` elements.
    pub fn condition_names(&self) -> Option<Vec<String>> {
        let mut res = Vec::new();

        for name in self
            .condition_list
            .read()
            .expect("cannot read condition registry")
            .keys()
        {
            res.push(name.clone())
        }
        if res.is_empty() { None } else { Some(res) }
    }

    /// Return the id of the specified condition
    pub fn condition_id(&self, name: &str) -> Option<i64> {
        if self.has_condition(name) {
            let cl0 = self
                .condition_list
                .read()
                .expect("cannot read condition registry");
            let cond = cl0.get(name).expect("cannot retrieve condition").clone();
            drop(cl0);
            let id = cond.lock().expect("cannot lock condition").get_id();
            Some(id)
        } else {
            None
        }
    }

    /// Check whether a condition is busy
    ///
    /// This function allows to test whether or not a condition is in a busy
    /// state at the moment of invocation. The synchronization system is
    /// exploited for this purpose and the result is not 100% reliable, as
    /// the condition could change its state immediately after the function
    /// has exited. However it is almost sure that this will not happen, as
    /// conditions are scheduled at discrete intervals and the purpose of this
    /// method is actually to help decide whether or not to rerun the `tick`
    /// operation.
    ///
    /// # Panics
    ///
    /// This function panics when called upon a name that does not exist in
    /// the registry
    pub fn condition_busy(&self, name: &str) -> bool {
        assert!(self.has_condition(name), "condition {name} not in registry");

        // since we return after trying to lock the condition, the possibly
        // acquired lock is immediately released
        if self.condition_is_free(name) {
            log(
                LogType::Trace,
                LOG_EMITTER_CONDITION_REGISTRY,
                LOG_ACTION_CONDITION_BUSY,
                None,
                LOG_WHEN_START,
                LOG_STATUS_OK,
                &format!("condition {name} is not busy"),
            );
            false
        } else {
            log(
                LogType::Trace,
                LOG_EMITTER_CONDITION_REGISTRY,
                LOG_ACTION_CONDITION_BUSY,
                None,
                LOG_WHEN_START,
                LOG_STATUS_FAIL,
                &format!("condition {name} is busy"),
            );
            true
        }
    }

    /// Check whether the condition can be locked (private use only)
    fn condition_is_free(&self, name: &str) -> bool {
        assert!(self.has_condition(name), "condition {name} not in registry");

        // what follows just *reads* the registry: the condition is retrieved
        // and the corresponding structure is operated in a way that mutates
        // only its inner state, and not the wrapping pointer
        let cl0 = self
            .condition_list
            .read()
            .expect("cannot read condition registry");
        let cond = cl0
            .get(name)
            .expect("cannot retrieve condition for busy check");
        let cond = cond.clone();
        drop(cl0);

        // since we return after trying to lock the condition, the possibly
        // acquired lock is immediately released
        let res = cond.try_lock();
        res.is_ok()
    }

    /// Report the number of busy conditions
    ///
    /// Report an unsigned integer corresponding to how many conditions are
    /// busy at the time of invocation: when the result is `Ok(Some(0))` there
    /// are no active condition tests and no active tasks.
    ///
    /// # Panics
    ///
    /// May panic if the busy condition count could not be locked.
    pub fn conditions_busy(&self) -> Result<Option<u64>> {
        let res: u64 = *self
            .conditions_busy
            .clone()
            .lock()
            .expect("cannot lock condition busy counter");
        Ok(Some(res))
    }

    /// Perform a condition test and run associated tasks if successful
    ///
    /// This function is called directly by the scheduler in order to actually
    /// perform the test associated to a registered condition and, if the test
    /// succeeds, execute the associated tasks according to the required order
    /// (if sequentially) or simultaneously. A result of `Ok(Some(true))` is
    /// returned if execution succeeded.
    ///
    /// # Arguments
    ///
    /// `name` - the name of the condition to check
    ///
    /// # Panics
    ///
    /// May panic if the condition registry could not be locked for extraction
    /// and if the provided name is not found in the registry: in no way the
    /// `tick` function should be invoked with unknown conditions.
    pub fn tick(&self, name: &str) -> Result<Option<bool>> {
        assert!(self.has_condition(name), "condition {name} not in registry");

        // what follows just *reads* the registry: the condition is retrieved
        // and the corresponding structure is operated in a way that mutates
        // only its inner state, and not the wrapping pointer
        let id = self.condition_id(name).unwrap();
        let cl0 = self
            .condition_list
            .read()
            .expect("cannot read condition registry");
        let cond = cl0
            .get(name)
            .expect("cannot retrieve condition for testing");
        let cond = cond.clone();
        drop(cl0);

        let mxc0 = cond.try_lock();
        if let Ok(mut cond) = mxc0 {
            log(
                LogType::Trace,
                LOG_EMITTER_CONDITION_REGISTRY,
                LOG_ACTION_TICK,
                Some((name, id)),
                LOG_WHEN_START,
                LOG_STATUS_MSG,
                &format!("test and run for condition {name}"),
            );

            // increment number of busy conditions by one: this can be done
            // without *self being mut because conditions_busy is an Arc;
            // we explicitly control the scope of the busy connection counter
            // in order to avoid that it is kept locked for even one cycle
            // more than it is strictly necessary
            let cb0 = self.conditions_busy.clone();
            let mut bcount = cb0.lock().expect("cannot lock condition busy counter");
            *bcount += 1;
            drop(bcount);
            drop(cb0);

            // TODO: this can be rewritten in a simpler way using the `?`
            // operator, thanks to the unified error system
            // the heart of all: test the condition and run tasks if verified
            let res = match cond.test() {
                Ok(o) => {
                    if let Some(outcome) = o {
                        if outcome { cond.run_tasks() } else { Ok(None) }
                    } else {
                        Ok(None)
                    }
                }
                Err(e) => Err(e),
            };

            // same as above regarding direct scope control
            let cb0 = self.conditions_busy.clone();
            let mut bcount = cb0.lock().expect("cannot lock condition busy counter");
            *bcount -= 1;
            drop(bcount);
            drop(cb0);

            // this cloned string is useful to look into queues
            let sname = String::from(name);

            // after this, check whether this condition is in one of the queues
            // of conditions to be suspended, or reset (or maybe removed? for
            // the moment, removal is still treated as a special case, similar
            // to insertion)
            let mxq0 = self.conditions_to_reset.clone();
            let mut queue = mxq0
                .lock()
                .expect("cannot acquire list of conditions to reset");
            if queue.contains(&sname) {
                if cond.reset().is_ok() {
                    log(
                        LogType::Debug,
                        LOG_EMITTER_CONDITION_REGISTRY,
                        LOG_ACTION_RESET_CONDITIONS,
                        None,
                        LOG_WHEN_PROC,
                        LOG_STATUS_OK,
                        &format!("condition {name} successfully reset"),
                    );
                    // since it has been reset we can remove it from the queue
                    let i = queue.iter().position(|x| *x == sname).unwrap();
                    queue.swap_remove(i);
                } else {
                    log(
                        LogType::Debug,
                        LOG_EMITTER_CONDITION_REGISTRY,
                        LOG_ACTION_RESET_CONDITIONS,
                        None,
                        LOG_WHEN_PROC,
                        LOG_STATUS_FAIL,
                        &format!("condition {name} could not be reset"),
                    );
                }
            }
            drop(queue);
            drop(mxq0);

            let mxq0 = self.conditions_to_suspend.clone();
            let mut queue = mxq0
                .lock()
                .expect("cannot acquire list of conditions to suspend");
            if queue.contains(&sname) {
                if cond.suspend().is_ok() {
                    log(
                        LogType::Debug,
                        LOG_EMITTER_CONDITION_REGISTRY,
                        LOG_ACTION_SUSPEND_CONDITION,
                        None,
                        LOG_WHEN_PROC,
                        LOG_STATUS_OK,
                        &format!("condition {name} successfully suspended"),
                    );
                    // since it has been suspended we can remove it from the queue
                    let i = queue.iter().position(|x| *x == sname).unwrap();
                    queue.swap_remove(i);
                } else {
                    log(
                        LogType::Debug,
                        LOG_EMITTER_CONDITION_REGISTRY,
                        LOG_ACTION_SUSPEND_CONDITION,
                        None,
                        LOG_WHEN_PROC,
                        LOG_STATUS_FAIL,
                        &format!("condition {name} could not be suspended"),
                    );
                }
            }
            drop(queue);
            drop(mxq0);

            // this is the right time to operate on the registry if there are
            // no busy conditions remaining, thus in this order:
            //
            // 1. remove conditions that must be uninstalled,
            // 2. add the conditions that have to be installed;
            //
            // note that locking the counter also prevents other tests to be
            // performed in other threads: this part must therefore be quick
            let cb0 = self.conditions_busy.clone();
            let bcount = cb0.lock().expect("cannot lock condition busy counter");
            if *bcount == 0 {
                // remove conditions
                let mxq0 = self.items_to_remove.clone();
                let mut queue = mxq0.lock().expect("cannot acquire list of items to remove");
                for name in queue.iter() {
                    if let Ok(item) = self.remove_condition(name) {
                        if let Some(item) = item {
                            let name = item.get_name();
                            log(
                                LogType::Debug,
                                LOG_EMITTER_CONDITION_REGISTRY,
                                LOG_ACTION_UNINSTALL,
                                None,
                                LOG_WHEN_PROC,
                                LOG_STATUS_OK,
                                &format!("successfully removed condition {name} from the registry"),
                            );
                        } else {
                            log(
                                LogType::Debug,
                                LOG_EMITTER_CONDITION_REGISTRY,
                                LOG_ACTION_UNINSTALL,
                                None,
                                LOG_WHEN_PROC,
                                LOG_STATUS_FAIL,
                                &format!("condition to remove {name} not found in the registry"),
                            );
                        }
                    }
                }
                queue.clear();
                drop(queue);
                drop(mxq0);
                // add conditions
                let mxq0 = self.items_to_add.clone();
                let mut queue = mxq0.lock().expect("cannot acquire list of items to add");
                while !queue.is_empty() {
                    if let Some(boxed_item) = queue.pop() {
                        let name = boxed_item.get_name();
                        if let Ok(res) = self.add_condition(boxed_item) {
                            let id = self.condition_id(&name).unwrap();
                            if res {
                                log(
                                    LogType::Debug,
                                    LOG_EMITTER_CONDITION_REGISTRY,
                                    LOG_ACTION_INSTALL,
                                    Some((&name, id)),
                                    LOG_WHEN_PROC,
                                    LOG_STATUS_OK,
                                    "successfully added queued condition to the registry",
                                );
                            } else {
                                log(
                                    LogType::Debug,
                                    LOG_EMITTER_CONDITION_REGISTRY,
                                    LOG_ACTION_INSTALL,
                                    Some((&name, id)),
                                    LOG_WHEN_PROC,
                                    LOG_STATUS_FAIL,
                                    "queued condition already present in the registry",
                                );
                            }
                        } else {
                            log(
                                LogType::Debug,
                                LOG_EMITTER_CONDITION_REGISTRY,
                                LOG_ACTION_INSTALL,
                                None,
                                LOG_WHEN_PROC,
                                LOG_STATUS_FAIL,
                                &format!("could not add queued condition {name} to the registry"),
                            );
                        }
                    }
                }
                drop(queue);
                drop(mxq0);
            }

            res
        } else {
            log(
                LogType::Debug,
                LOG_EMITTER_CONDITION_REGISTRY,
                LOG_ACTION_TICK,
                Some((name, id)),
                LOG_WHEN_START,
                LOG_STATUS_MSG,
                &format!("condition {name} is busy: skipping tick"),
            );
            Ok(None)
        }
    }
}

// end.
