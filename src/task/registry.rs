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


use std::sync::Arc;
use std::sync::Mutex;
use std::sync::RwLock;
use std::sync::mpsc::channel;
use std::thread::JoinHandle;
use std::thread::spawn;
use std::collections::HashMap;
use std::io::{Error, ErrorKind};

use lazy_static::lazy_static;
use unique_id::Generator;
use unique_id::sequence::SequenceGenerator;

use super::base::Task;
use crate::common::logging::{log, LogType};
use crate::constants::*;


// module-wide values
lazy_static! {
    // the main task ID generator
    static ref UID_GENERATOR: SequenceGenerator = {
        let mut _uidgen = SequenceGenerator;
        _uidgen
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
    task_list: RwLock<HashMap<String, Arc<Mutex<Box<dyn Task>>>>>,
}


#[allow(dead_code)]
impl TaskRegistry {

    /// Create a new, empty `TaskRegistry`.
    pub fn new() -> Self {
        TaskRegistry {
            task_list: RwLock::new(HashMap::new()),
        }
    }

    /// Check whether or not a task with the provided name is in the registry.
    ///
    /// # Arguments
    ///
    /// * name - the name of the task to check for registration
    ///
    /// # Panics
    ///
    /// May panic if the task registry could not be locked for enquiry.
    pub fn has_task(&self, name: &str) -> bool {
        self.task_list
            .read()
            .expect("cannot read task registry")
            .contains_key(name)
    }

    /// Check whether all tasks in a list are in the registry (**Note**: this
    /// function is mostly used internally for verification), returns `true`
    /// only if _all_ tasks in the list are found in the registry.
    ///
    /// # Arguments
    ///
    /// * names - a list of task names (as a vector)
    ///
    /// # Panics
    ///
    /// May panic if the task registry could not be locked for enquiry.
    pub fn has_all_tasks(&self, names: &Vec<&str>) -> bool {
        for name in names {
            if !self.task_list
                .read()
                .expect("cannot read task registry")
                .contains_key(*name) {
                return false;
            }
        }
        true
    }

    /// Add an already-boxed `Task` if its name is not present in the registry.
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
    ///                  provided to the function as a `Box<dyn Task>`
    ///
    /// # Returns
    ///
    /// * `Ok(true)` - the task could be added to the registry
    /// * `Ok(false)` - the task could not be inserted
    ///
    /// **Note**: the task is _moved_ into the registry, and can only be
    ///           released (and given back stored in a `Box`) using the
    ///           `remove_task`function. Also, although the possible outcomes
    ///           include an error condition, `Err(_)` is never returned.
    ///
    /// # Panics
    ///
    /// May panic if the task registry could not be locked for insertion.
    pub fn add_task(&self, mut boxed_task: Box<dyn Task>) -> Result<bool, std::io::Error> {
        let name = boxed_task.get_name();
        if self.has_task(&name) {
            return Ok(false);
        }
        // only consume an ID if the task is not discarded, otherwise the
        // released task would be safe to run even when not registered
        boxed_task.set_id(generate_task_id());
        self.task_list
            .write()
            .expect("cannot write to task registry")
            .insert(name, Arc::new(Mutex::new(boxed_task)));
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
    /// * `Error(ErrorKind::Unsupported, _)` - the task could not be removed
    /// * `Ok(None)` - task not found in registry
    /// * `Ok(Task)` - the removed (_pulled out_) `Task` on success
    ///
    /// # Panics
    ///
    /// May panic if the task registry could not be locked for extraction.
    pub fn remove_task(&self, name: &str) -> Result<Option<Box<dyn Task>>, std::io::Error> {
        if self.has_task(name) {
            if let Some(r) = self.task_list
                .write()
                .expect("cannot write to task registry")
                .remove(name) {
                let Ok(mx) = Arc::try_unwrap(r) else {
                    panic!("attempt to extract referenced task {name}")
                };
                let mut task = mx
                    .into_inner()
                    .expect("cannot extract locked task");
                task.set_id(0);
                Ok(Some(task))
            } else {
                Err(Error::new(
                    ErrorKind::Unsupported,
                    ERR_TASKREG_CANNOT_PULL_TASK,
                ))
            }
        } else {
            Ok(None)
        }
    }


    /// Return the list of task names as owned strings.
    ///
    /// Return a vector containing the names of all the tasks that have been
    /// registered, as `String` elements.
    pub fn task_names(&self) -> Option<Vec<String>> {
        let mut res = Vec::new();

        for name in self.task_list
            .read()
            .expect("cannot read task registry")
            .keys() {
            res.push(name.clone())
        }
        if res.is_empty() {
            None
        } else {
            Some(res)
        }
    }

    /// Return the id of the specified task
    pub fn task_id(&self, name: &str) -> Option<i64> {
        let guard;
        if self.has_task(name) {
            guard = self.task_list
                .read()
                .expect("cannot read task registry");
        } else {
            return None
        }
        let task = guard
            .get(name)
            .expect("cannot retrieve task")
            .clone();
        drop(guard);
        let id = task.lock().expect("cannot lock task").get_id();
        Some(id)
    }


    /// Run a list of tasks sequentially.
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
    /// task names that are unknown. Also, it panics when the registry could
    /// not be locked for task retrieval.
    pub fn run_tasks_seq(
        &self,
        trigger_name: &str,
        names: &Vec<&str>,
        break_failure: bool,
        break_success: bool,
    ) -> HashMap<String, Result<Option<bool>, std::io::Error>> {
        let mut res: HashMap<String, Result<Option<bool>, std::io::Error>> = HashMap::new();

        if !self.has_all_tasks(names) {
            panic!("(trigger {trigger_name}): run_tasks_seq task(s) not found in registry")
        }

        // although this function runs a task sequentially, we must handle the
        // task registry in the same way as if the tasks were concurrent: in
        // fact there might be other branches accessing the registry right at
        // the same moment when this sequence is running
        for name in names.iter() {
            let id = self.task_id(name).unwrap();
            let mut breaks = false;
            let task;
            let guard = self.task_list
                .read()
                .expect("cannot lock task registry");
            task = guard
                .get(*name)
                .expect("cannot retrieve task for running")
                .clone();
            drop(guard);
            let cur_res;
            let mut guard = task
                .lock()
                .expect("cannot lock task while extracting");
            cur_res = guard.run(trigger_name);
            log(
                LogType::Trace,
                LOG_EMITTER_TASK_REGISTRY,
                "run_seq",
                Some((name, id)),
                LOG_WHEN_END,
                LOG_STATUS_MSG,
                &format!("task {name} finished running"),
            );
            drop(guard);

            let mut task_success = false;
            if let Ok(outcome) = cur_res {
                if let Some(success) = outcome {
                    task_success = success;
                    if (success && break_success) || (!success && break_failure) {
                        breaks = true;
                    }
                } else if break_failure {   // error is considered a failure
                    breaks = true;
                }
            }
            res.insert(String::from(*name), cur_res);
            if breaks {
                log(
                    LogType::Debug,
                    LOG_EMITTER_TASK_REGISTRY,
                    "run_seq",
                    Some((name, id)),
                    LOG_WHEN_END,
                    LOG_STATUS_MSG,
                    &format!("breaking on {}", { if task_success { "success" } else { "failure" } }),
                );
                break;
            }
        }

        log(
            LogType::Trace,
            LOG_EMITTER_TASK_REGISTRY,
            "run_seq",
            None,
            LOG_WHEN_END,
            LOG_STATUS_MSG,
            &format!("finished running {}/{} tasks", res.len(), names.len()),
        );
        res
    }

    /// Run a list of tasks simultaneously.
    ///
    /// Executes all the tasks in the provided list simultaneously, each in a
    /// separate thread, and waiting for all tasks to finish. The name of a
    /// `Condition` trigger must be provided for logging purposes. The outcomes
    /// are returned in a `HashMap`.
    ///
    /// **Note:** this function runs in the calling thread, that is blocked
    /// until it returns.
    ///
    /// TODO: this must be reimplemented with a maximum concurrency level,
    ///       possibly using a _thread pool_.
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
    /// task names that are unknown. Also, it panics when the registry could
    /// not be locked for task retrieval.
    pub fn run_tasks_par(
        &self,
        trigger_name: &str,
        names: &Vec<&str>,
    ) -> HashMap<String, Result<Option<bool>, std::io::Error>> {
        if !self.has_all_tasks(names) {
            panic!("(trigger: {trigger_name}) run_tasks_par task(s) not found in registry")
        }

        let mut handles: Vec<JoinHandle<()>> = Vec::new();
        let mut res: HashMap<String, Result<Option<bool>, std::io::Error>> = HashMap::new();

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
            let guard = self.task_list
                .read()
                .expect("cannot lock task registry");
            let task = guard
                .get(*name)
                .expect("cannot retrieve task for running")
                .clone();
            drop(guard);

            let aname = Arc::new(String::from(*name));
            let atrname = atrname.clone().to_string();
            let atx = atx.clone();
            let handle = spawn(move || {
                let outcome = task
                    .lock()
                    .expect("cannot lock task for running")
                    .run(&atrname);
                atx.lock().unwrap().send((aname.clone(), outcome)).unwrap();
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
                "run_par",
                None,
                LOG_WHEN_END,
                LOG_STATUS_ERR,
                &format!("not all outcomes received ({outcomes_received}/{outcomes_total})"),
            );
        } else {
            log(
                LogType::Debug,
                LOG_EMITTER_TASK_REGISTRY,
                "run_par",
                None,
                LOG_WHEN_END,
                LOG_STATUS_MSG,
                &format!("all outcomes received ({outcomes_received}/{outcomes_total})"),
            );
        }
        res
    }

}


// end.
