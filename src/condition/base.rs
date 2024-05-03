//! Define a trait for conditions
//!
//! Conditions must follow the constraint of having a `_check_condition`
//! function that return a `Result<Option<bool>, std::io::Error>`, where the
//! condition is considered verified only if it returns `Ok(true)`. This
//! mandatory checker is used by the trait to perform actual tests.
//!
//! Conditions have a very complex set of internal flags to indicate various
//! states, including:
//!
//! * _suspension_ - whether or not a `Condition` has to be checked
//! * _recurrency_ - a `Condition` can be either _one-shot_ or recurring
//! * _checkedness_ - whether or not a `Condition` has ever been checked after
//!                   last reset
//! * _successfulness_ - whether or not last check result was a success since
//!                      last verification
//!
//! Note that being _suspended_ for a `Condition` is different from being
//! _active_: an _active_ condition is such if it has been registered in the
//! `Condition` registry, and if inactive it will never be checked anyway. A
//! _suspended_ condition will not be tested even if it is registered (and thus
//! _active_).


use std::time::Instant;
use std::io::{Error, ErrorKind};
use crate::common::logging::{log, LogType};

use crate::constants::*;
use crate::task::registry::TaskRegistry;



/// Define the interface for `Condition` objects.
///
/// **Note**: the methods prefixed with an underscore must be defined in
///           the types implementing the trait, but *must not* be used
///           by the trait object users.
#[allow(dead_code)]
pub trait Condition: Send {

    /// Mandatory ID setter for registration.
    fn set_id(&mut self, id: i64);

    /// Return the name of the `Condition` as an _owned_ `String`.
    fn get_name(&self) -> String;

    /// Return the ID of the `Condition`.
    fn get_id(&self) -> i64;

    /// Get the condition type as a string reference.
    fn get_type(&self) -> &str;

    /// Mandatory task registry setter.
    fn set_task_registry(&mut self, reg: &'static TaskRegistry);

    /// Mandatory task registry getter.
    fn task_registry(&self) -> Option<&'static TaskRegistry>;


    /// Return `true` if the condition is _suspended_.
    fn suspended(&self) -> bool;

    /// Return `true` if the condition is _recurring_, otherwise a one-shot.
    fn recurring(&self) -> bool;

    /// Return `true` if the condition succeeded the last time it was tested.
    fn has_succeeded(&self) -> bool;


    /// Return `true` if associated tasks should run sequentially.
    fn exec_sequence(&self) -> bool;

    /// Return `true` if associated task sequence should break on first success.
    fn break_on_success(&self) -> bool;

    /// Return `true` if associated task sequence should break on first failure.
    fn break_on_failure(&self) -> bool;


    /// Return last test time (if any).
    fn last_checked(&self) -> Option<Instant>;

    /// Return last success time (if any).
    fn last_succeeded(&self) -> Option<Instant>;

    /// Return the time in which condition tests were started (if they were).
    fn startup_time(&self) -> Option<Instant>;


    /// Set the internal _checked_ state to `true`.
    fn set_checked(&mut self) -> Result<bool, std::io::Error>;

    /// Set the internal _succeeded_ state to `true`.
    fn set_succeeded(&mut self) -> Result<bool, std::io::Error>;

    /// Set the internal _succeeded_ state to `false`.
    fn reset_succeeded(&mut self) -> Result<bool, std::io::Error>;

    /// Fully reset internal state of the condition.
    fn reset(&mut self) -> Result<bool, std::io::Error>;


    /// Set the startup time to `Instant::now()`.
    fn start(&mut self) -> Result<bool, std::io::Error>;

    /// Set the internal _suspended_ state to `true`.
    fn suspend(&mut self) -> Result<bool, std::io::Error>;

    /// Set the internal _suspended_ state to `false`.
    fn resume(&mut self) -> Result<bool, std::io::Error>;


    /// Get a list of task names as owned strings.
    fn task_names(&self) -> Result<Vec<String>, std::io::Error>;

    /// Check whether or not there are associated tasks.
    fn has_tasks(&self) -> Result<bool, std::io::Error> {
        Ok(!self.task_names()?.is_empty())
    }

    /// Verify last outcome after checking the `Condition`.
    ///
    /// Reports whether or not the result of last check was a success, in
    /// which case `Ok(true)` is returned. Otherwise returns `Ok(false)`.
    /// This function can be used only once after a check: it resets the
    /// internal _succeeded_ status for next call. May return an error if
    /// it wasn't possible to reset the internal _succeeded_ status.
    fn verify(&mut self) -> Result<bool, std::io::Error> {
        if let Some(tc) = self.last_checked() {
            if let Some(ts) = self.last_succeeded() {
                let res = ts == tc;
                self.reset_succeeded()?;
                return Ok(res);
            }
        }
        Ok(false)
    }


    /// Mandatory check function.
    ///
    /// This function, in objects implementing the `Condition` trait, actually
    /// performs the check, and must return a successfulness value in the form
    /// of a `Result<Option<bool>, std::io::Error>`. The return value is
    /// interpreted as follows:
    ///
    /// * `Ok(Some(true))` - the _only_ case considered a success
    /// * `Ok(Some(false))` - a verified failure in the test
    /// * `Ok(None)` - indefinite state, it normally should indicate that it
    ///                was impossible to check the condition; yields a failure
    /// * `Err(_)` - an error occurred and will be logged, failure anyway
    ///
    /// The different return states are logged accordingly by the trait-defined
    /// `test` function, indirectly invoked by the scheduler.
    fn _check_condition(&mut self) -> Result<Option<bool>, std::io::Error>;


    /// Mandatory to add task names: should return `Ok(true)` on success
    fn _add_task(&mut self, name: &str) -> Result<bool, std::io::Error>;

    /// Mandatory to remove task names: should return `Ok(true)` on success
    fn _remove_task(&mut self, name: &str) -> Result<bool, std::io::Error>;


    /// Log a message in the specific `Condition` format.
    ///
    /// This utility is provided so that all conditions can log in a consistent
    /// format, and has to be used for logging avoiding other kinds of output.
    /// The severity is provided as a parameter and must be one of the values
    /// listed in `common::LogType`.
    ///
    /// # Arguments
    ///
    /// * `severity` - one of `LogType::{Trace, Debug, Info, Warn, Error}`
    /// * `message` - the message to be logged as a borrowed string
    fn log(&self, severity: LogType, when: &str, status: &str, message: &str) {
        let name = self.get_name();
        let id = self.get_id();
        log(
            severity,
            LOG_EMITTER_CONDITION,
            LOG_ACTION_ACTIVE,
            Some((name.as_str(), id)),
            when,
            status,
            message,
        );
    }


    /// Interface to condition checks.
    ///
    /// This method calls the provided check function (`_check_condition`) and
    /// returns the check result. The possible return  values are:
    ///
    /// * `Ok(Some(true))` - the _only_ case considered a success
    /// * `Ok(Some(false))` - a verified failure in the test
    /// * `Ok(None)` - indefinite state, it normally should indicate that it
    ///                was impossible to check the condition; yields a failure
    /// * `Err(_)` - an error occurred and will be logged, failure anyway
    ///
    /// The result of the `_check_condition` function is logged, as well as
    /// other possible issues that prevent the test. Only `Ok(Some(true))` is
    /// considered a success.
    ///
    /// # Panics
    ///
    /// This method can only be invoked on **registered** conditions, any
    /// attempt to perform a test on an unregistered condition must be
    /// considered a development error.
    fn test(&mut self) -> Result<Option<bool>, std::io::Error> {
        // panic if the condition has not yet been registered
        if self.get_id() == 0 {
            panic!("condition {} not registered", self.get_name());
        }

        // bail out if the condition has no associated tasks, if it
        // is suspended, or if it has been successful once and is not
        // set to be recurrent, and check otherwise
        if !self.has_tasks().unwrap_or(false) {
            self.log(
                LogType::Debug,
                LOG_WHEN_PROC,
                LOG_STATUS_MSG,
                "skipping check: condition has no associated tasks",
            );
            Ok(None)
        }
        else if self.suspended() {
            self.log(
                LogType::Debug,
                LOG_WHEN_PROC,
                LOG_STATUS_MSG,
                "skipping check: condition is suspended",
            );
            Ok(None)
        } else if self.has_succeeded() && !self.recurring() {
            self.log(
                LogType::Debug,
                LOG_WHEN_PROC,
                LOG_STATUS_MSG,
                "skipping check: condition is not recurring",
            );
            Ok(None)
        } else {
            if !self.reset_succeeded()? {
                self.log(
                    LogType::Error,
                    LOG_WHEN_PROC,
                    LOG_STATUS_FAIL,
                    "aborting: condition could not reset success status",
                );
                return Err(Error::new(
                    ErrorKind::Unsupported,
                    ERR_COND_CANNOT_RESET,
                ));
            }
            self.log(
                LogType::Trace,
                LOG_WHEN_PROC,
                LOG_STATUS_OK,
                "checking condition",
            );

            // call the inner mandatory checker
            if self.set_checked()? {
                if let Some(outcome) = self._check_condition()? {
                    if outcome {
                        if self.set_succeeded()? {
                            self.log(
                                LogType::Info,
                                LOG_WHEN_PROC,
                                LOG_STATUS_OK,
                                "success: condition checked with positive outcome",
                            );
                        } else {
                            self.log(
                                LogType::Error,
                                LOG_WHEN_PROC,
                                LOG_STATUS_FAIL,
                                "aborting: condition could not be set to succeeded",
                            );
                            return Err(Error::new(
                                ErrorKind::Unsupported,
                                ERR_COND_CANNOT_SET_SUCCESS,
                            ));
                        }
                    } else {
                        self.log(
                            LogType::Info,
                            LOG_WHEN_PROC,
                            LOG_STATUS_OK,
                            "failure: condition checked with negative outcome",
                        );
                    }
                    Ok(Some(outcome))
                } else {
                    self.log(
                        LogType::Warn,
                        LOG_WHEN_PROC,
                        LOG_STATUS_FAIL,
                        "exiting: condition provided NO outcome",
                    );
                    Ok(None)
                }
            } else {
                self.log(
                    LogType::Error,
                    LOG_WHEN_PROC,
                    LOG_STATUS_FAIL,
                    "aborting: condition could not be set to checked",
                );
                return Err(Error::new(
                    ErrorKind::Unsupported,
                    ERR_COND_CANNOT_SET_CHECKED,
                ));
            }
        }
    }


    /// Add a task to this `Condition`.
    ///
    /// Associate a task to the condition: the task name is _appended_ to the
    /// list of associated tasks, thus determining the execution order if the
    /// tasks have to be executed sequentially. A `Task` name can only be
    /// appended if it has been registered. Returns `Ok(true)` if the task
    /// could be added, any other return value indicates a failure.
    fn add_task(&mut self, name: &str) -> Result<bool, std::io::Error> {
        // check that the task is actually in the regstry
        self.log(
            LogType::Trace,
            LOG_WHEN_INIT,
            LOG_STATUS_MSG,
            &format!("checking for presence of task {name} in the registry"),
        );

        // check for presence in registry and issue an error if not
        match self.task_registry() {
            Some(r) => {
                if !r.has_task(name) {
                    self.log(
                        LogType::Error,
                        LOG_WHEN_INIT,
                        LOG_STATUS_FAIL,
                        &format!("could not add task {name}: not found in registry"),
                    );
                    return Err(Error::new(
                        ErrorKind::Unsupported,
                        ERR_COND_TASK_NOT_ADDED,
                    ));
                }
            }
            None => {
                self.log(
                    LogType::Error,
                    LOG_WHEN_INIT,
                    LOG_STATUS_FAIL,
                    &format!("could not add task {name}: registry not assigned"),
                );
                return Err(Error::new(
                    ErrorKind::Unsupported,
                    ERR_COND_TASK_NOT_ADDED,
                ));
            }
        }

        // actually try to add the task
        let outcome = self._add_task(name)?;
        if outcome {
            self.log(
                LogType::Debug,
                LOG_WHEN_INIT,
                LOG_STATUS_OK,
                &format!("task {name} successfully added to condition"),
            );

        } else {
            self.log(
                LogType::Error,
                LOG_WHEN_INIT,
                LOG_STATUS_FAIL,
                &format!("could not add task {name} to condition"),
            );
        }

        Ok(outcome)
    }

    /// Remove a task from this `Condition`.
    ///
    /// Deassociate a task from the condition: the task name is removed from
    /// the list of associated tasks. Returns `Ok(true)` if the task could be
    /// removed, any other return value indicates a failure.
    fn remove_task(&mut self, name: &str) -> Result<bool, std::io::Error> {
        // actually try to remove the task
        let outcome = self._remove_task(name)?;
        if outcome {
            self.log(
                LogType::Debug,
                LOG_WHEN_INIT,
                LOG_STATUS_OK,
                &format!("task {name} successfully removed from condition"),
            );

        } else {
            self.log(
                LogType::Error,
                LOG_WHEN_INIT,
                LOG_STATUS_FAIL,
                &format!("could not remove task {name} from condition"),
            );
        }

        Ok(outcome)
    }


    /// Run the associated tasks.
    ///
    /// The assocaiated tasks are run, either sequentially or simultaneously
    /// according to condition configuration. Task execution is logged as well
    /// as task outcomes after execution.
    ///
    /// **Note**: This function waits for all tasks to finish prior to
    ///           returning: spawning a separate thread may be needed.
    ///
    /// # Panics
    ///
    /// This method can only be invoked on **registered** conditions, any
    /// attempt to run tasks associated to an unregistered condition must be
    /// considered a development error.
    fn run_tasks(&mut self) -> Result<Option<bool>, std::io::Error> {
        // panic if the condition has not yet been registered
        if self.get_id() == 0 {
            panic!("condition {} not registered", self.get_name());
        }

        let registry = self.task_registry().unwrap();
        let mut s_task_names = String::new();
        let names = self.task_names()?;

        if !names.is_empty() {
            for name in names.clone().iter() {
                if !s_task_names.is_empty() {
                    s_task_names = format!("{s_task_names} {name}");
                } else {
                    s_task_names = format!("{name}");
                }
            }
        } else {
            self.log(
                LogType::Warn,
                LOG_WHEN_PROC,
                LOG_STATUS_FAIL,
                "no tasks found associated to condition",
            );
            return Ok(None);
        }

        let res = if self.exec_sequence() {
            self.log(
                LogType::Info,
                LOG_WHEN_PROC,
                LOG_STATUS_OK,
                &format!("running tasks sequentially: {s_task_names}"),
            );
            registry.run_tasks_seq(
                &self.get_name(),
                &names.iter().map(|s| s.as_str()).collect(),
                self.break_on_failure(),
                self.break_on_success(),
            )
        } else {
            self.log(
                LogType::Info,
                LOG_WHEN_PROC,
                LOG_STATUS_OK,
                &format!("running tasks simultaneously: {s_task_names}"),
            );
            registry.run_tasks_par(
                &self.get_name(),
                &names.iter().map(|s| s.as_str()).collect(),
            )
        };

        for name in res.keys() {
            match res.get(name).unwrap() {
                Ok(outcome) => {
                    if let Some(outcome) = outcome {
                        self.log(
                            LogType::Info,
                            LOG_WHEN_PROC,
                            LOG_STATUS_OK,
                            &format!(
                                "task {name} completed: outcome is {}",
                                { if *outcome {"success"} else {"failure"} },
                            )
                        );
                    } else {
                        self.log(
                            LogType::Info,
                            LOG_WHEN_PROC,
                            LOG_STATUS_OK,
                            &format!("task {name} completed"),
                        );
                    }
                }
                Err(err) => {
                    self.log(
                        LogType::Warn,
                        LOG_WHEN_PROC,
                        LOG_STATUS_OK,
                        &format!("task {name} exited with error: {err}"),
                    );
                }
            }
        }

        self.log(
            LogType::Debug,
            LOG_WHEN_PROC,
            LOG_STATUS_OK,
            &format!("finished running tasks: {s_task_names}"),
        );

        Ok(Some(true))
    }

}


// end.
