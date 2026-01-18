//! # Task Registry
//!
//! `task::registry` implements the main registry for `Task` objects.
//!
//! Implements the task registry as the main interface to access and execute
//! _active_ tasks: a `Task` object cannot in fact be considered active until
//! it is _registered_. A registered task has an unique nonzero ID (the `Task`
//! trait does not allow running a task when it does not have an ID), and it
//! can be executed in a series either sequentially or simultaneously with
//! other tasks, respectively using the `run_tasks_seq` and `run_tasks_par`
//! functions.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::RwLock;
use std::sync::mpsc::channel;
use std::thread::JoinHandle;
use std::thread::spawn;

use lazy_static::lazy_static;
use unique_id::Generator;
use unique_id::sequence::SequenceGenerator;

use super::base::{Task, TaskRef};
use crate::common::logging::{LogType, log};
use crate::common::wres::{Error, Kind, Result};
use crate::constants::*;

// module-wide values
lazy_static! {
    // the main task ID generator
    static ref UID_GENERATOR: SequenceGenerator = {

        SequenceGenerator
    };
}

// the specific task ID generator: used internally to register a task
#[allow(dead_code)]
fn generate_task_id() -> i64 {
    UID_GENERATOR.next_id()
}

/// The task registry: there must be one and only one task registry in each
/// instance of the process, and should have `'static` lifetime. It may be
/// passed around as a reference for tasks.
pub struct TaskRegistry {
    // the entire list is enclosed in `RwLock<...>` in order to avoid
    // concurrent access to the list itself
    task_list: RwLock<HashMap<String, Arc<Mutex<TaskRef>>>>,

    // counter to verify whether there are running tasks at a moment
    running_sessions: Arc<Mutex<u64>>,

    // the two queues for items to remove and items to add: the items that
    // need to be added are stored as full (dyn) items, while the ones to
    // be removed are stored as names
    items_to_remove: Arc<Mutex<Vec<String>>>,
    items_to_add: Arc<Mutex<Vec<TaskRef>>>,
}

#[allow(dead_code)]
impl TaskRegistry {
    /// Create a new, empty `TaskRegistry`.
    pub fn new() -> Self {
        TaskRegistry {
            task_list: RwLock::new(HashMap::new()),
            running_sessions: Arc::new(Mutex::new(0)),

            items_to_remove: Arc::new(Mutex::new(Vec::new())),
            items_to_add: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Check whether or not a task with the provided name is in the registry.
    ///
    /// # Arguments
    ///
    /// * name - the name of the task to check for registration
    pub fn has_task(&self, name: &str) -> Result<bool> {
        Ok(self.task_list.read()?.contains_key(name))
    }

    /// Check whether or not the provided task is in the registry.
    ///
    /// # Arguments
    ///
    /// * name - the name of the task to check for registration
    pub fn has_task_eq(&self, task: &dyn Task) -> Result<bool> {
        let name = task.get_name();
        if self.has_task(name.as_str())? {
            let tasks = self.task_list.read()?;
            let found_task = tasks.get(name.as_str()).unwrap();
            let t0 = found_task.clone();
            let locked_task = t0.lock()?;
            return Ok(locked_task.eq(task));
        }

        Ok(false)
    }

    /// Check whether all tasks in a list are in the registry
    ///
    /// **Note**: this function is mostly used internally for verification,
    /// it returns `true` only if _all_ tasks in the list are found in the
    /// registry.
    ///
    /// # Arguments
    ///
    /// * names - a list of task names (as a vector).
    pub fn has_all_tasks(&self, names: &Vec<&str>) -> Result<bool> {
        for name in names {
            if !self.task_list.read()?.contains_key(*name) {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Add already-boxed `Task` if its name is not present in the registry
    ///
    /// The `Box` ensures that the enclosed task is transferred as a reference
    /// and stored as-is in the registry. Note that for the registration to be
    /// successful there must **not** already be a task with the same name in
    /// the registry: if such task is found `Ok(false)` is returned. In order
    /// to replace a `Task` it has to be removed first, then added.
    ///
    /// # Arguments
    ///
    /// * `boxed_task` - an object implementing the `base::Task` trait,
    ///   provided to the function as a `Box<dyn Task>` aka `TaskRef`
    ///
    /// # Returns
    ///
    /// * `Ok(true)` - the task could be added to the registry
    /// * `Ok(false)` - the task could not be inserted
    ///
    /// **Note**: the task is _moved_ into the registry, and can only be
    /// released (and given back stored in a `Box`) using the `remove_task`
    /// function.
    pub fn add_task(&self, mut boxed_task: TaskRef) -> Result<bool> {
        let name = boxed_task.get_name();
        if self.has_task(&name)? {
            return Ok(false);
        }
        // only consume an ID if the task is not discarded, otherwise the
        // released task would be safe to run even when not registered
        boxed_task.set_id(generate_task_id());
        self.task_list
            .write()?
            .insert(name, Arc::new(Mutex::new(boxed_task)));
        Ok(true)
    }

    /// Add or replace an already-boxed `Task` while running: if the registry
    /// is busy running any task all modifications are deferred
    pub fn dynamic_add_or_replace_task(&self, boxed_task: TaskRef) -> Result<bool> {
        let name = boxed_task.get_name();
        let sessions = self.running_sessions.clone();
        let sessions = sessions.lock()?;
        if *sessions == 0 {
            if self.has_task(&name)? {
                match self.remove_task(&name) {
                    Ok(_) => {
                        if let Ok(res) = self.add_task(boxed_task) {
                            return Ok(res);
                        } else {
                            return Err(Error::new(Kind::Failed, ERR_TASKREG_TASK_NOT_REPLACED));
                        }
                    }
                    _ => {
                        return Err(Error::new(Kind::Failed, ERR_TASKREG_CANNOT_PULL_TASK));
                    }
                }
            } else if let Ok(res) = self.add_task(boxed_task) {
                return Ok(res);
            } else {
                return Err(Error::new(Kind::Failed, ERR_TASKREG_TASK_NOT_ADDED));
            }
        } else {
            let queue = self.items_to_add.clone();
            let mut queue = queue.lock()?;
            queue.push(boxed_task);
            log(
                LogType::Debug,
                LOG_EMITTER_TASK_REGISTRY,
                LOG_ACTION_NEW,
                None,
                LOG_WHEN_PROC,
                LOG_STATUS_OK,
                &format!("registry busy: task {name} set to be added when no tasks are running"),
            );
        }

        Ok(true)
    }

    /// Remove a named task from the list and give it back stored in a Box.
    ///
    /// The returned `Task` can be modified and stored back in the registry:
    /// before returning, the boxed `Task` is 'uninitialized' (that is, its
    /// ID is set back to 0) so that it wouldn't run if asked to; the rest of
    /// its internal status is preserved.
    ///
    /// # Arguments
    ///
    /// * `name` - the name of the task that must be removed
    ///
    /// # Returns
    ///
    /// * `Error(Kind::Failed, _)` - the task could not be removed
    /// * `Ok(None)` - task not found in registry
    /// * `Ok(Task)` - the removed (_pulled out_) `Task` on success.
    pub fn remove_task(&self, name: &str) -> Result<Option<TaskRef>> {
        if self.has_task(name)? {
            match self.task_list.write()?.remove(name) {
                Some(r) => {
                    let Ok(mx) = Arc::try_unwrap(r) else {
                        return Err(Error::new(Kind::Failed, ERR_ACCESS_FAILED));
                    };
                    let mut task = mx.into_inner()?;
                    task.set_id(0);
                    Ok(Some(task))
                }
                _ => Err(Error::new(Kind::Failed, ERR_TASKREG_CANNOT_PULL_TASK)),
            }
        } else {
            Ok(None)
        }
    }

    /// Remove a named task from the list operating on a running registry: if any
    /// tasks are running all modifications to the registry are deferred
    pub fn dynamic_remove_task(&self, name: &str) -> Result<bool> {
        if self.has_task(name)? {
            let sessions = self.running_sessions.clone();
            let sessions = sessions.lock()?;
            if *sessions == 0 {
                self.remove_task(name)?;
            } else {
                let queue = self.items_to_remove.clone();
                let mut queue = queue.lock()?;
                queue.push(String::from(name));
                log(
                    LogType::Debug,
                    LOG_EMITTER_TASK_REGISTRY,
                    LOG_ACTION_UNINSTALL,
                    None,
                    LOG_WHEN_PROC,
                    LOG_STATUS_OK,
                    &format!(
                        "registry busy: task {name} set to be removed when no tasks are running",
                    ),
                );
            }
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Return the list of task names as owned strings
    ///
    /// Return a vector containing the names of all the tasks that have been
    /// registered, as `String` elements.
    pub fn task_names(&self) -> Result<Option<Vec<String>>> {
        let mut res = Vec::new();

        for name in self.task_list.read()?.keys() {
            res.push(name.clone())
        }
        if res.is_empty() {
            Ok(None)
        } else {
            Ok(Some(res))
        }
    }

    /// Return the id of the specified task
    pub fn task_id(&self, name: &str) -> Result<Option<i64>> {
        if self.has_task(name).unwrap() {
            // TO_FIX
            let tl0 = self.task_list.read()?;
            let task = tl0.get(name).expect("cannot retrieve task").clone();
            drop(tl0);
            let id = task.lock()?.get_id();
            Ok(Some(id))
        } else {
            Ok(None)
        }
    }

    /// Run a list of tasks sequentially
    ///
    /// Executes a list of tasks, provided as reference to a `Vec` of names, in
    /// the order in which the names are stored in the list. The execution may
    /// stop on the first success or failure outcome, provided that the
    /// appropriate flag argument (respectively `break_success` and
    /// `break_failure`) is set to `true`. Of course both can be set to `true`,
    /// as there may be tasks that exit with an indefinite state. The name of a
    /// `Condition` trigger must be provided for logging purposes. The outcomes
    /// are returned in a `HashMap`.
    ///
    /// **Note:** this function runs in the calling thread, that is blocked
    /// until it returns.
    ///
    /// # Arguments
    ///
    /// * `trigger_name` - the name of the triggering `Condition`
    /// * `names` - a vector containing the names of the tasks
    /// * `break_failure` - if set break on first failure
    /// * `break_success` - if set break on first success
    ///
    /// # Returns
    ///
    /// A `HashMap` whose keys are the names of the tasks _that have been run_
    /// (that is, may not be the entire list of provided names) and whose
    /// elements are their respective outcomes.
    ///
    /// # Panics
    ///
    /// If one or more task names are not in the registry the function panics:
    /// in no way there should be the option that this function is invoked with
    /// task names that are unknown.
    pub fn run_tasks_seq(
        &self,
        trigger_name: &str,
        names: &Vec<&str>,
        break_failure: bool,
        break_success: bool,
    ) -> Result<HashMap<String, Result<Option<bool>>>> {
        assert!(
            self.has_all_tasks(names)?,
            "some tasks not found in registry for condition `{trigger_name}`"
        );

        let mut res: HashMap<String, Result<Option<bool>>> = HashMap::new();

        // count the active running sessions: there can be more than a
        // command task session in execution at the moment, and knowing
        // that none is running anymore is necessary to handle changes
        // in the list of registered tasks
        let sessions = self.running_sessions.clone();

        // increase the number of running sessions before running tasks
        {
            let mut sessions = sessions.lock()?;
            *sessions += 1;
        }

        // although this function runs a task sequentially, we must handle the
        // task registry in the same way as if the tasks were concurrent: in
        // fact there might be other branches accessing the registry right at
        // the same moment when this sequence is running
        for name in names.iter() {
            let id = self.task_id(name)?.unwrap();
            let mut breaks = false;
            let task;
            let tl0 = self.task_list.read()?;
            task = tl0
                .get(*name)
                .expect("cannot retrieve task for running")
                .clone();
            drop(tl0);

            let mut t0 = task.lock()?;
            let cur_res = t0.run(trigger_name);
            log(
                LogType::Trace,
                LOG_EMITTER_TASK_REGISTRY,
                LOG_ACTION_RUN_TASKS_SEQ,
                Some((name, id)),
                LOG_WHEN_END,
                LOG_STATUS_MSG,
                &format!("task {name} finished running"),
            );
            drop(t0);

            let mut task_success = false;
            if let Ok(outcome) = cur_res {
                if let Some(success) = outcome {
                    task_success = success;
                    if (success && break_success) || (!success && break_failure) {
                        breaks = true;
                    }
                } else if break_failure {
                    // error is considered a failure
                    breaks = true;
                }
            }
            res.insert(String::from(*name), cur_res);
            if breaks {
                log(
                    LogType::Debug,
                    LOG_EMITTER_TASK_REGISTRY,
                    LOG_ACTION_RUN_TASKS_SEQ,
                    Some((name, id)),
                    LOG_WHEN_END,
                    LOG_STATUS_MSG,
                    &format!("breaking on {}", {
                        if task_success { "success" } else { "failure" }
                    }),
                );
                break;
            }
        }

        // decrease the number of running sessions: if the number reaches zero
        // then perform the item add/removal routine while the session counter
        // is locked, so that no one else can modify the current items list;
        // note that since the counter is locked, no other sessions can be run
        // in other possible threads
        {
            let mut sessions = sessions.lock()?;
            *sessions -= 1;

            if *sessions == 0 {
                let rm_queue = self.items_to_remove.clone();
                {
                    let queue = rm_queue.lock()?;
                    for name in queue.iter() {
                        if let Ok(item) = self.remove_task(name) {
                            if let Some(item) = item {
                                let name = item.get_name();
                                log(
                                    LogType::Debug,
                                    LOG_EMITTER_TASK_REGISTRY,
                                    LOG_ACTION_UNINSTALL,
                                    None,
                                    LOG_WHEN_PROC,
                                    LOG_STATUS_OK,
                                    &format!("successfully removed task {name} from the registry"),
                                );
                            } else {
                                log(
                                    LogType::Debug,
                                    LOG_EMITTER_TASK_REGISTRY,
                                    LOG_ACTION_UNINSTALL,
                                    None,
                                    LOG_WHEN_PROC,
                                    LOG_STATUS_OK,
                                    &format!("task to remove {name} not found in the registry"),
                                );
                            }
                        }
                    }
                }
                let add_queue = self.items_to_add.clone();
                {
                    let mut queue = add_queue.lock()?;
                    while !queue.is_empty() {
                        if let Some(boxed_item) = queue.pop() {
                            let name = boxed_item.get_name();
                            if let Ok(res) = self.add_task(boxed_item) {
                                let id = self.task_id(&name)?.unwrap();
                                if res {
                                    log(
                                        LogType::Debug,
                                        LOG_EMITTER_TASK_REGISTRY,
                                        LOG_ACTION_INSTALL,
                                        Some((&name, id)),
                                        LOG_WHEN_PROC,
                                        LOG_STATUS_OK,
                                        "successfully added queued task to the registry",
                                    );
                                } else {
                                    log(
                                        LogType::Debug,
                                        LOG_EMITTER_TASK_REGISTRY,
                                        LOG_ACTION_INSTALL,
                                        Some((&name, id)),
                                        LOG_WHEN_PROC,
                                        LOG_STATUS_FAIL,
                                        "queued task already present in the registry",
                                    );
                                }
                            } else {
                                log(
                                    LogType::Debug,
                                    LOG_EMITTER_TASK_REGISTRY,
                                    LOG_ACTION_INSTALL,
                                    None,
                                    LOG_WHEN_PROC,
                                    LOG_STATUS_FAIL,
                                    &format!("could not add queued task {name} to the registry"),
                                );
                            }
                        }
                    }
                }
            }
        }

        log(
            LogType::Trace,
            LOG_EMITTER_TASK_REGISTRY,
            LOG_ACTION_RUN_TASKS_SEQ,
            None,
            LOG_WHEN_END,
            LOG_STATUS_MSG,
            &format!("finished running {}/{} tasks", res.len(), names.len()),
        );
        Ok(res)
    }

    /// Run a list of tasks simultaneously
    ///
    /// Executes all the tasks in the provided list simultaneously, each in a
    /// separate thread, and waiting for all tasks to finish. The name of a
    /// `Condition` trigger must be provided for logging purposes. The outcomes
    /// are returned in a `HashMap`.
    ///
    /// **Note:** this function runs in the calling thread, that is blocked
    /// until it returns.
    ///
    /// TODO: this might be reimplemented with a maximum concurrency level,
    /// possibly using a _thread pool_.
    ///
    /// # Arguments
    ///
    /// * `trigger_name` - the name of the triggering `Condition`
    /// * `names` - a vector containing the names of the tasks
    ///
    /// # Returns
    ///
    /// A `HashMap` whose keys are the names of the tasks and whose elements
    /// are their respective outcomes.
    ///
    /// # Panics
    ///
    /// If one or more task names are not in the registry the function panics:
    /// in no way there should be the option that this function is invoked with
    /// task names that are unknown.
    pub fn run_tasks_par(
        &self,
        trigger_name: &str,
        names: &Vec<&str>,
    ) -> Result<HashMap<String, Result<Option<bool>>>> {
        assert!(
            self.has_all_tasks(names)?,
            "some tasks not found in registry for condition `{trigger_name}`"
        );

        // count the active running sessions: there can be more than a
        // command task session in execution at the moment, and knowing
        // that none is running anymore is necessary to handle changes
        // in the list of registered tasks
        let sessions = self.running_sessions.clone();

        // increase the number of running sessions before running tasks
        {
            let mut sessions = sessions.lock()?;
            *sessions += 1;
        }

        // this version of the runner is obviously multithreaded
        let mut handles: Vec<JoinHandle<()>> = Vec::new();
        let mut res: HashMap<String, Result<Option<bool>>> = HashMap::new();

        // the channel is used to communicate with spawned task threads
        let (tx, rx) = channel();
        let atx = Arc::new(Mutex::new(tx));

        // spawn all tasks (almost) simultaneously: the spawned code only uses
        // Arc-ed references to common data, which are freed anyway when this
        // scope exits - that is after all threads have joined; this function
        // in fact waits for all the threads to finish, and has to be called
        // in a separate thread from the main thread
        let atrname = Arc::new(trigger_name);

        for name in names.iter() {
            // the task list is only *read*: this greatly simplifies handling of
            // strings used as indexes, in this case the task name
            let tl0 = self.task_list.read()?;
            let task = tl0
                .get(*name)
                .expect("cannot retrieve task for running")
                .clone();
            drop(tl0);

            let aname = Arc::new(String::from(*name));
            let atrname = atrname.clone().to_string();
            let atx = atx.clone();
            let handle = spawn(move || {
                if let Ok(mut locked_task) = task.lock() {
                    let outcome = locked_task.run(&atrname);
                    if let Ok(atx) = atx.lock() {
                        let _ = atx.send((aname.clone(), outcome));
                    }
                }
            });
            handles.push(handle);
        }

        // wait for all threads to finish prior to returning to caller
        for handle in handles.into_iter() {
            let _ = handle.join();
        }

        // get all results back from the threads and build the result map
        let outcomes_total = names.len();
        let mut outcomes_received = 0;
        for _ in 0..outcomes_total {
            if let Ok((k, v)) = rx.recv() {
                res.insert(k.to_string(), v);
                outcomes_received += 1;
            }
        }

        // report if any of the outcomes could not be retrieved (is an error)
        if outcomes_received < outcomes_total {
            log(
                LogType::Warn,
                LOG_EMITTER_TASK_REGISTRY,
                LOG_ACTION_RUN_TASKS_PAR,
                None,
                LOG_WHEN_END,
                LOG_STATUS_ERR,
                &format!("not all outcomes received ({outcomes_received}/{outcomes_total})"),
            );
        } else {
            log(
                LogType::Trace,
                LOG_EMITTER_TASK_REGISTRY,
                LOG_ACTION_RUN_TASKS_PAR,
                None,
                LOG_WHEN_END,
                LOG_STATUS_MSG,
                &format!("all outcomes received ({outcomes_received}/{outcomes_total})"),
            );
        }

        // decrease the number of running sessions: if the number reaches zero
        // then perform the item add/removal routine while the session counter
        // is locked, so that no one else can modify the current items list
        {
            let mut sessions = sessions.lock()?;
            *sessions -= 1;

            if *sessions == 0 {
                let rm_queue = self.items_to_remove.clone();
                {
                    let queue = rm_queue.lock()?;
                    for name in queue.iter() {
                        if let Ok(item) = self.remove_task(name) {
                            if let Some(item) = item {
                                let name = item.get_name();
                                log(
                                    LogType::Debug,
                                    LOG_EMITTER_TASK_REGISTRY,
                                    LOG_ACTION_UNINSTALL,
                                    None,
                                    LOG_WHEN_PROC,
                                    LOG_STATUS_OK,
                                    &format!("successfully removed task {name} from the registry"),
                                );
                            } else {
                                log(
                                    LogType::Debug,
                                    LOG_EMITTER_TASK_REGISTRY,
                                    LOG_ACTION_UNINSTALL,
                                    None,
                                    LOG_WHEN_PROC,
                                    LOG_STATUS_OK,
                                    &format!("task to remove {name} not found in the registry"),
                                );
                            }
                        }
                    }
                }
                let add_queue = self.items_to_add.clone();
                {
                    let mut queue = add_queue.lock()?;
                    while !queue.is_empty() {
                        if let Some(boxed_item) = queue.pop() {
                            let name = boxed_item.get_name();
                            if let Ok(res) = self.add_task(boxed_item) {
                                let id = self.task_id(&name)?.unwrap();
                                if res {
                                    log(
                                        LogType::Debug,
                                        LOG_EMITTER_TASK_REGISTRY,
                                        LOG_ACTION_NEW,
                                        Some((&name, id)),
                                        LOG_WHEN_PROC,
                                        LOG_STATUS_OK,
                                        "successfully added queued task to the registry",
                                    );
                                } else {
                                    log(
                                        LogType::Debug,
                                        LOG_EMITTER_TASK_REGISTRY,
                                        LOG_ACTION_NEW,
                                        Some((&name, id)),
                                        LOG_WHEN_PROC,
                                        LOG_STATUS_FAIL,
                                        "queued task already present in the registry",
                                    );
                                }
                            } else {
                                log(
                                    LogType::Debug,
                                    LOG_EMITTER_TASK_REGISTRY,
                                    LOG_ACTION_NEW,
                                    None,
                                    LOG_WHEN_PROC,
                                    LOG_STATUS_FAIL,
                                    &format!("could not add queued task {name} to the registry"),
                                );
                            }
                        }
                    }
                }
            }
        }

        Ok(res)
    }
}

// end.
