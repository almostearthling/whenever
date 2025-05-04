//! Define a trait for tasks
//!
//! Tasks must only respect the constraint of having an accessible `_run`
//! method that takes the name of a triggering condition as argument and
//! returns an outcome in the form of a `Result<Option<bool>, Error>` (see
//! the `run` function documentation for details).
//!
//! Also, a `Task` must give read access to its name in the form of a string,
//! and read/write access to its ID in the form of an unsigned integer. A zero
//! ID is used for _inactive_ tasks.

use crate::common::logging::{log, LogType};
use crate::common::wres::Result;
use crate::constants::*;

/// Define the interface for `Task` objects.
///
/// **Note**: the methods prefixed with an underscore must be defined in
///           the types implementing the trait, but *must not* be used
///           by the trait object users.
#[allow(dead_code)]
pub trait Task: Send {
    /// Mandatory ID setter for registration.
    fn set_id(&mut self, id: i64);

    /// Return the name of the `Task` as an _owned_ `String`.
    fn get_name(&self) -> String;

    /// Return the ID of the `Task`.
    fn get_id(&self) -> i64;

    /// Tell whether or not another `Task` is equal to this
    fn eq(&self, other: &dyn Task) -> bool {
        self._hash() == other._hash()
    }

    /// Tell whether or not another `Task` is not equal to this
    fn ne(&self, other: &dyn Task) -> bool {
        !self.eq(other)
    }

    /// Internally called to implement `eq()` and `neq()`: hash calculation
    /// is costly in terms of time and possibly CPU, but it is supposed to
    /// take place very seldomly, near to almost never
    fn _hash(&self) -> u64;

    /// Internally called to actually execute the `Task`.
    fn _run(&mut self, trigger_name: &str) -> Result<Option<bool>>;

    /// Log a message in the specific `Task` format.
    ///
    /// This utility is provided so that all tasks can log in a consistent
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
            LOG_EMITTER_TASK,
            LOG_ACTION_ACTIVE,
            Some((&name, id)),
            when,
            status,
            message,
        );
    }

    /// Execute this `Task`.
    ///
    /// Executes the main `Task` function, returning an outcome. The outcome
    /// _must_ have the form of a `Result`:
    ///
    /// * the `Ok` part must be either `None`, or a `bool` that reports success
    ///   when set to `true` and failure otherwise; when `None` it indicates
    ///   that the result should not be checked
    /// * the `Error` part indicates an _unrecoverable_ error condition while
    ///   trying to execute the task: itactually should never happen, even when
    ///   the task reports a failure (which should return `Ok(false)` instead).
    ///
    /// This function is the only responsible for _history records_, the log
    /// records used to mimic the _history_ feature seen in the Python version
    /// (**When**): in fact these records, although human readable, are very
    /// brief and expected to be used by GUI/TUI wrappers.
    ///
    /// # Panics
    ///
    /// This function panics if the task is not **registered**: any call to an
    /// unregistered task must be considered a development error. Tasks can
    /// actually only be started via the registry.
    fn run(&mut self, trigger_name: &str) -> Result<Option<bool>> {
        assert!(
            self.get_id() != 0,
            "task {} not registered",
            self.get_name()
        );

        self.log(
            LogType::Trace,
            LOG_WHEN_HISTORY,
            LOG_STATUS_HIST_START,
            &format!("OK/trigger:{trigger_name} starting task"),
        );
        let res = self._run(trigger_name);
        match &res {
            Ok(v) => {
                if let Some(b) = v {
                    if *b {
                        self.log(
                            LogType::Trace,
                            LOG_WHEN_HISTORY,
                            LOG_STATUS_HIST_END,
                            &format!("OK/trigger:{trigger_name} task succeeded"),
                        );
                    } else {
                        self.log(
                            LogType::Trace,
                            LOG_WHEN_HISTORY,
                            LOG_STATUS_HIST_END,
                            &format!("FAIL/trigger:{trigger_name} task failed"),
                        );
                    }
                } else {
                    self.log(
                        LogType::Trace,
                        LOG_WHEN_HISTORY,
                        LOG_STATUS_HIST_END,
                        &format!("IND/trigger:{trigger_name} no outcome"),
                    );
                }
            }
            Err(e) => {
                self.log(
                    LogType::Trace,
                    LOG_WHEN_HISTORY,
                    LOG_STATUS_HIST_END,
                    &format!("ERR/trigger:{trigger_name} error: {}", e),
                );
            }
        }
        res
    }
}

// end.
