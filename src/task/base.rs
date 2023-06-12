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


/// Define the interface for `Task` objects.
///
/// **Note**: the methods prefixed with an underscore must be defined in
///           the types implementing the trait, but *must not* be used
///           by the trait object users.
pub trait Task: Send {

    /// Mandatory ID setter for registration.
    fn set_id(&mut self, id: i64);

    /// Return the name of the `Task` as an _owned_ `String`.
    fn get_name(&self) -> String;

    /// Return the ID of the `Task`.
    fn get_id(&self) -> i64;

    /// Internally called to actually execute the task.
    fn _run(
        &mut self,
        trigger_name: &str,
    ) -> Result<Option<bool>, std::io::Error>;

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
    fn log(&self, severity: LogType, message: &str) {
        let name = self.get_name().clone();
        let id = self.get_id();
        log(severity, &format!("TASK {name}/[{id}]"), message);
    }


    /// Execute this `Task`.
    ///
    /// Executes the main `Task` function, returning an outcome. The outcome
    /// _must_ have the form of a `Result`:
    ///
    /// * the `Ok` part must be either `None`, or a `bool` that reports success
    ///   when set to `true` and failure otherwise; when `None` it indicates
    ///   that the result should not be checked
    /// * the `Error` part (mandatorily `std::io::Error`) indicates an
    ///   _unrecoverable_ error condition while trying to execute the task: it
    ///   actually should never happen, even when the task reports a failure
    ///   (which should return `Ok(false)` instead).
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
    fn run(
        &mut self,
        trigger_name: &str,
    ) -> Result<Option<bool>, std::io::Error> {
        // panic if the task has not yet been registered
        if self.get_id() == 0 {
            panic!("(trigger {trigger_name}): task {} not registered",
                self.get_name(),
            );
        }

        self.log(
            LogType::Trace, 
            &format!("[HIST/START]:OK/trigger:{trigger_name} starting task"),
        );
        let res = self._run(trigger_name);
        match &res {
            Ok(v) => {
                if let Some(b) = v {
                    if *b {
                        self.log(
                            LogType::Trace, 
                            &format!("[HIST/END]:OK/trigger:{trigger_name} task succeeded"),
                        );
                    } else {
                        self.log(
                            LogType::Trace, 
                            &format!("[HIST/END]:FAIL/trigger:{trigger_name} task failed"),
                        );
                    }
                } else {
                    self.log(
                        LogType::Trace, 
                        &format!("[HIST/END]:IND/trigger:{trigger_name} no outcome"),
                    );
                }
            }
            Err(e) => {
                self.log(
                    LogType::Trace, 
                    &format!("[HIST/END]:ERR/trigger:{trigger_name} error: {}", e.to_string()),
                );
            }
        }
        res
    }

}



// end.
