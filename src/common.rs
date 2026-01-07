//! Common modules and other globally available items
//!
//! The common logging system is a simplified version of what is available in
//! the `log` crate, and all logging functions shall use this common module.
//!
//! Some notes on logging:
//!
//! * The log messages are composed by
//!     - the timestamp
//!     - the application name (see below) in brackets
//!     - the log level
//!     - the log message
//! * The log message in turn has the following form:
//!   `context: [MSGTYPE] human readable message`
//!   where
//!     - the context is usually constructed with two space-separated strings
//!       indicating the part of the program where a certain message is issued
//!     - MSGTYPE (in square brackets) consists of two or more alphanumeric
//!       strings, separated by slashes, whose first two are described below
//!       and the further ones may depend on the first two
//!     - the human readable message is an explanation of what happened.
//!
//! The first two elements in MSGTYPE indicate in which point of an operation
//! the event occurs, and the type of event. The first element can be one of:
//!
//! * _INIT_ if the event occurs in an initialization phase
//! * _START_ if the event occurs while starting something, service or process
//! * _PROC_ if the event occurs while processing or during some activity
//! * _END_ if the event occurs at the end of a service or process
//! * _HIST_ is a _trace level only_ message emitted to show history on GUI:
//!   in this case _MSG_ is sent at the beginning of task execution, and
//!   _OK_, _FAIL_ or _IND_ are sent at the end (resp. on success,
//!   failure or _indeterminate_ outcome)
//! * _BUSY_ is also a _trace level only_ message emitted to allow a GUI to
//!   show the application status (for instance using an icon in the
//!   tray area): when there are one or more conditions busy, the second
//!   element is _YES_, otherwise _NO_
//! * _PAUSE_ another _trace level only_ message emitted to allow a GUI to
//!   change application status (for instance using a tray icon) when
//!   the scheduler is paused: useful because an _internal command_
//!   based task might pause the scheduler unattendedly
//!
//! while the second can be one of:
//!
//! * _OK_ for expected outcomes or behaviours
//! * _FAIL_ for unexpected outcomes or behaviours
//! * _IND_ for indeterminate outcomes
//! * _MSG_ if the human-readable part is exclusively informational
//! * _ERR_ (may be followed by a dash `-` and a code) for errors to be
//!   notified
//! * _YES_ (only occurs for _BUSY_ or _PAUSE_ indicators) means: application
//!   is busy or has been paused
//! * _NO_ (only occurs for _BUSY_ or _PAUSE_ indicators) means: application
//!   is _not_ busy or has been resumed
//!
//! This should help using the log as a way of communicating to a wrapper
//! utility the state of the scheduler, and possibily give the opportunity to
//! organize communication to the user in a friendlier way.
//!
//! This module also contains common enums, traits, structs, and functions
//! shared between items that use the same technology. Shared collections are
//! organized in modules:
//!
//! * `cmditem` for assets common to command based tasks and conditions
//! * `luaitem` for assets common to Lua based tasks and conditions
//! * `dbusitem` for assets common to DBus based conditions and events
//! * `wmiitem` for assets common to WMI based conditions and events
//! * `wres` for the _whenever_ specific `Result`, that has automations
//!   for conversion from many other result types
//!
//! in order to avoid behaviour discrepancies, and possibly to save some
//! memory by avoiding unnecessary duplications.

use lazy_static::lazy_static;
use std::sync::RwLock;

// the following global flag is exposed here because it looks like there is
// no actual way to pass anything but a string as payload to the logger, so
// the common logging function should know whether the logger is initialized
// to return JSON message and build the JSON payload itself
lazy_static! {
    static ref LOGGER_EMITS_JSON: RwLock<bool> = RwLock::new(false);
}

#[allow(dead_code)]
/// Module for logging
///
/// Exposes (publicly):
///
/// * a function to universally log (`log`)
/// * a logger initialization function (`init`)
/// * the logging levels: _trace_ < _debug_ < _info_ < _warn_ < _error_,
///   provided as the `LogType` enumeration
pub mod logging {
    use crate::constants::{APP_NAME, ERR_LOGGER_NOT_INITIALIZED};
    use flexi_logger::{DeferredNow, FileSpec, Logger, style};
    use log::Record;
    use log::{debug, error, info, trace, warn};
    use nu_ansi_term::Style;
    use serde_json::json;
    use std::path::PathBuf;

    use super::LOGGER_EMITS_JSON;

    // time stamp format that is used by the provided format functions.
    const NOW_FMT: &str = "%Y-%m-%dT%H:%M:%S%.3f";
    const NOW_FMT_FULL: &str = "%Y-%m-%dT%H:%M:%S%.6f";

    // log formatters
    fn log_format_plain(
        w: &mut dyn std::io::Write,
        now: &mut DeferredNow,
        record: &Record,
    ) -> Result<(), std::io::Error> {
        write!(
            w,
            "[{}] ({APP_NAME}) {} {}",
            now.format(NOW_FMT),
            format_args!("{:5}", record.level()),
            &record.args(),
        )
    }

    fn log_format_json(
        w: &mut dyn std::io::Write,
        now: &mut DeferredNow,
        record: &Record,
    ) -> Result<(), std::io::Error> {
        let header = json!({
            "application": APP_NAME,
            "time": now.format(NOW_FMT_FULL).to_string(),
            "level": record.level().to_string(),
        });
        let payload = record.args();
        write!(w, "{{\"header\":{header},\"contents\":{payload}}}")
    }

    fn log_format_colors(
        w: &mut dyn std::io::Write,
        now: &mut DeferredNow,
        record: &Record,
    ) -> Result<(), std::io::Error> {
        let level = record.level();
        let bold = Style::new().bold();
        let dimmed = Style::new().dimmed();
        write!(
            w,
            "[{}] {} {} {}",
            format_args!("{}", now.format(NOW_FMT)),
            dimmed.paint(format!("({APP_NAME})")),
            style(level).paint(format!("{:5}", level.to_string())),
            bold.paint(record.args().to_string()),
        )
    }

    /// Log levels (from most verbose to least)
    pub enum LogType {
        Trace,
        Debug,
        Info,
        Warn,
        Error,
    }

    /// Logger initialization: if `filename` is not given, the log will be
    /// sent to stdout and use color (and the `append` parameter will be
    /// ignored); otherwise `filename` will be used as path for the log file:
    /// causes an error if it's not possible to open the log file.
    pub fn init(
        level: LogType,
        filename: Option<String>,
        append: bool,
        logcolor: bool, // these three values are mutually
        logplain: bool, // exclusive by construction of the
        logjson: bool,  // main `clap` parser
    ) -> std::io::Result<bool> {
        let level = match level {
            LogType::Trace => "trace",
            LogType::Debug => "debug",
            LogType::Info => "info",
            LogType::Warn => "warn",
            LogType::Error => "error",
        };

        // the following line is to avoid other crates logging (e.g. `zbus`)
        // so it can be commented out for debugging purposes and replaced with
        // the subsequent commented out line. A reminder to documentation:
        // https://docs.rs/flexi_logger/latest/flexi_logger/struct.LogSpecification.html
        // FIXME: maybe we can choose the actual configuration string
        //        automatically according to the current build settings?
        let logspec = format!("whenever={level}");
        // let logspec = format!("{level}");

        let mut logger;
        logger = Logger::try_with_str(logspec);
        match logger {
            Ok(l) => {
                if let Some(fname) = filename {
                    let log_format = {
                        if logcolor {
                            log_format_plain
                        } else if logplain {
                            log_format_plain
                        } else if logjson {
                            *LOGGER_EMITS_JSON.write().unwrap() = true;
                            log_format_json
                        } else {
                            log_format_plain
                        }
                    };
                    let mut pb = PathBuf::from(&fname);
                    if pb.parent().is_none()
                        || pb.parent().unwrap().to_str().unwrap_or("").is_empty()
                    {
                        pb = {
                            let mut dir = PathBuf::from(".");
                            dir.push(pb);
                            dir
                        }
                    }
                    let fspec = FileSpec::try_from(&pb).unwrap();
                    logger = Ok(
                        l.log_to_file(fspec).format_for_files(log_format), // .write_mode(WriteMode::BufferAndFlush)
                    );
                    if append {
                        logger = Ok(logger.unwrap().append());
                    }
                } else {
                    let log_format = {
                        if logcolor {
                            log_format_colors
                        } else if logplain {
                            log_format_plain
                        } else if logjson {
                            *LOGGER_EMITS_JSON.write().unwrap() = true;
                            log_format_json
                        } else {
                            log_format_colors
                        }
                    };
                    // in json mode to console we also support capture by pipes
                    // so that wrappers may use stdout to get updates; it also
                    // redirects the logger's own errors to a black hole in
                    // order to avoid polluting a wrapper
                    if logjson {
                        logger = Ok(l
                            .format_for_stdout(log_format)
                            .write_mode(flexi_logger::WriteMode::Direct)
                            .error_channel(flexi_logger::ErrorChannel::DevNull)
                            .log_to_stdout());
                    } else {
                        logger = Ok(l.format_for_stdout(log_format).log_to_stdout());
                    }
                }
            }
            _ => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Unsupported,
                    ERR_LOGGER_NOT_INITIALIZED,
                ));
            }
        }
        if let Err(_e) = logger.unwrap().start() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                ERR_LOGGER_NOT_INITIALIZED,
            ));
        }

        Ok(true)
    }

    /// Common log function. The parameters are granular in order to achieve
    /// two benefits: the first is that for most of them a constant can be
    /// used, thus reducing the possibility of non-conformant log messages
    /// (which may arise on typos) and, to some extent, the executable size;
    /// the second is that JSON log messages can be as fine-grained as
    /// needed. The constants to be used are defined in _constants.rs_, and
    /// in particular:
    ///
    /// * `emitter` is one of the `LOG_EMITTER_...` constants
    /// * `action` is one of the `LOG_ACTION_...` constants
    /// * `when` is one of the `LOG_WHEN_...` constants
    /// * `status` is one of the `LOG_STATUS_...` constants
    ///
    /// while non-constant parameters must be defined as follows
    ///
    /// * `item` can be a tuple consisting of item _name_ and _id_
    /// * `message` is the only arbitrary string that can be passed
    ///
    /// This allows JSON messages to be easily interpretable by a wrapper
    /// according to the hints given in the documentation.
    pub fn log(
        severity: LogType,
        emitter: &str,
        action: &str,
        item: Option<(&str, i64)>,
        when: &str,
        status: &str,
        message: &str,
    ) {
        let payload = if *LOGGER_EMITS_JSON.read().unwrap() {
            let context = if let Some((item, item_id)) = item {
                json!({
                    "emitter": emitter,
                    "action": action,
                    "item": item,
                    "item_id": item_id,
                })
            } else {
                json!({
                    "emitter": emitter,
                    "action": action,
                    "item": null,
                    "item_id": null,
                })
            };
            let message_type = json!({
                "when": when,
                "status": status,
            });
            json!({
                "context": context,
                "message_type": message_type,
                "message": message,
            })
            .to_string()
        } else {
            let item_repr = if let Some((name, id)) = item {
                format!(" {name}/{id}")
            } else {
                String::new()
            };
            format!("{emitter} {action}{item_repr}: [{when}/{status}] {message}")
        };
        match severity {
            LogType::Trace => {
                trace!("{payload}")
            }
            LogType::Debug => {
                debug!("{payload}")
            }
            LogType::Info => {
                info!("{payload}")
            }
            LogType::Warn => {
                warn!("{payload}")
            }
            LogType::Error => {
                error!("{payload}")
            }
        }
    }
}

#[allow(dead_code)]
/// This module helps command based items perform common activities
pub mod cmditem {
    use std::time::{Duration, SystemTime};
    use subprocess::{ExitStatus, Popen};

    use crate::LogType;
    use crate::constants::*;

    /// In case of failure, the reason will be one of the provided values
    #[derive(Debug, PartialEq)]
    pub enum FailureReason {
        NoFailure,
        StdOut,
        StdErr,
        Status,
        Other,
    }

    /// Helper to start a process (in the same thread), read stdout/stderr
    /// continuously (thus freeing its buffers), optionally terminate it after
    /// a certain timeout has been reached: it returns a tuple consisting of
    /// status and, optionally, strings containing stdout and stderr contents.
    ///
    /// The process to be spawned must be created _before_ invoking the helper,
    /// thus it is a caller's responsibility to provide a ready-to-run process
    /// with open output channels, as the `proc` parameter. `poll_interval` is
    /// the time interval that interleaves subsequent reads of _stdout_ and
    /// _stderr_, and `timeout`, if any, is the time that will be waited for
    /// before terminating the subprocess. No way is provided to feed input to
    /// the subprocess.
    ///
    /// This helper is used by:
    ///
    /// * `task::command_task::CommandTask::_run()`
    /// * `condition::command_cond::CommandCondition::_check_condition()`
    pub fn spawn_process(
        mut proc: Popen,
        poll_interval: Duration,
        timeout: Option<Duration>,
    ) -> Result<(ExitStatus, Option<String>, Option<String>), std::io::Error> {
        let mut stdout = String::new();
        let mut stderr = String::new();
        let mut out;
        let mut err;
        let mut exit_status;
        let mut comm = proc.communicate_start(None).limit_time(poll_interval);
        let startup = SystemTime::now();

        loop {
            // we intercept timeout error here because we just could be waiting
            // for more output to be available; the timed_out flag is used to
            // avoid waiting extra time when reading from stdout/stderr has
            // already had a cost in this terms
            let mut timed_out = false;
            let cres = comm.read_string();
            if let Err(e) = &cres {
                if e.kind() == std::io::ErrorKind::TimedOut {
                    let (co, ce) = e.capture.clone();
                    timed_out = true;
                    if let Some(co) = co {
                        out = Some(String::from_utf8(co).unwrap_or_default());
                    } else {
                        out = None;
                    }
                    if let Some(ce) = ce {
                        err = Some(String::from_utf8(ce).unwrap_or_default());
                    } else {
                        err = None;
                    }
                } else {
                    return Err(std::io::Error::new(
                        e.kind(),
                        e.to_string(),
                    ));
                }
            } else {
                (out, err) = cres.unwrap();
            }

            if let Some(ref o) = out {
                stdout.push_str(o.as_str());
            }
            if let Some(ref e) = err {
                stderr.push_str(e.as_str());
            }
            exit_status = proc.poll();
            if exit_status.is_none() {
                if let Some(t) = timeout {
                    if SystemTime::now() > startup + t {
                        let res = proc.terminate();
                        if res.is_err() {
                            let _ = proc.kill();
                        }
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::TimedOut,
                            ERR_TIMEOUT_REACHED,
                        ));
                    }
                }
            } else {
                break;
            }
            if !timed_out {
                std::thread::sleep(poll_interval);
            }
        }

        // same as above
        let cres = comm.read_string();
        if let Err(e) = &cres {
            if e.kind() == std::io::ErrorKind::TimedOut {
                let (co, ce) = e.capture.clone();
                if let Some(co) = co {
                    out = Some(String::from_utf8(co).unwrap_or_default());
                } else {
                    out = None;
                }
                if let Some(ce) = ce {
                    err = Some(String::from_utf8(ce).unwrap_or_default());
                } else {
                    err = None;
                }
            } else {
                return Err(std::io::Error::new(
                    e.kind(),
                    e.to_string(),
                ));
            }
        } else {
            (out, err) = cres.unwrap();
        }
        if let Some(ref o) = out {
            stdout.push_str(o);
        }
        if let Some(ref e) = err {
            stderr.push_str(e);
        }
        if let Some(exit_status) = exit_status {
            Ok((
                exit_status,
                {
                    if !stdout.is_empty() {
                        Some(stdout)
                    } else {
                        None
                    }
                },
                {
                    if !stderr.is_empty() {
                        Some(stderr)
                    } else {
                        None
                    }
                },
            ))
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                ERR_UNKNOWN_EXITSTATUS,
            ))
        }
    }

    /// Check process outcome for items that spawn processes (that is: command
    /// based tasks and command based conditions), by taking refeence to most
    /// of their parameters and returning a tuple containing the check result:
    ///
    /// * the exit status
    /// * whether the process failed or not
    /// * the failure reason as a `FailureReason`
    ///
    /// and what is needed to build a log message:
    ///
    /// * the severity of the log line
    /// * the _when_ part of the log line
    /// * the _status_ part of the log line
    /// * the payload (human readable) message of the log line
    ///
    /// Of course the caller is responsible for interpretation of the return
    /// value and for logging the result of hte check, as well as setting the
    /// internal status of the item.
    ///
    /// This helper is quite inelegant: being used in two items, it is probably
    /// not worth to create a trait that is common to the two that can be used
    /// to pass the item directly -- even because _zero cost abstraction_ would
    /// probably duplicate the compiled code in this case, in order to achieve
    /// efficiency, which is exactly the opposite of what the condensation was
    /// intended for!
    ///
    /// This helper is used by:
    ///
    /// * `task::command_task::CommandTask::_run()`
    /// * `condition::command_cond::CommandCondition::_check_condition()`
    pub fn check_process_outcome(
        exit_status: &ExitStatus,
        process_stdout: &str,
        process_stderr: &str,
        command_line: &str,

        // from item configuration: flags
        match_exact: bool,
        match_regexp: bool,
        case_sensitive: bool,

        // from item configuration: expected outcomes
        success_stdout: &Option<String>,
        success_stderr: &Option<String>,
        success_status: &Option<u32>,
        failure_stdout: &Option<String>,
        failure_stderr: &Option<String>,
        failure_status: &Option<u32>,
    ) -> (
        u32,           // process_status
        bool,          // process_failed
        FailureReason, // failure_reason
        LogType,       // the log severity
        &'static str,  // log/when (LOG_WHEN_...)
        &'static str,  // log/status (LOG_STATUS_...)
        String,        // log message
    ) {
        let mut process_status: u32 = 0;
        let mut process_failed: bool = false;
        let mut failure_reason: FailureReason = FailureReason::NoFailure;
        let mut severity: LogType;
        let mut ref_log_when: &str;
        let mut ref_log_status: &str;
        let mut log_message: String;

        let statusmsg: String;
        if exit_status.success() {
            // exit code is 0, and this usually indicates success however if it
            // was not the expected exit code the failure reason has to be set
            // to Status (for now); note that also the case of exit code 0
            // considered as a failure status is taken into account here
            statusmsg = String::from("OK/0");
            severity = LogType::Debug;
            ref_log_when = LOG_WHEN_PROC;
            ref_log_status = LOG_STATUS_OK;
            log_message =
                format!("command: `{command_line}` exited with SUCCESS status {statusmsg}",);
            if let Some(expected) = success_status {
                if *expected != 0 {
                    severity = LogType::Debug;
                    ref_log_when = LOG_WHEN_PROC;
                    ref_log_status = LOG_STATUS_OK;
                    log_message =
                        format!("condition expected success exit code NOT matched: {expected}");
                    failure_reason = FailureReason::Status;
                }
            } else if let Some(expectedf) = failure_status {
                if *expectedf == 0 {
                    severity = LogType::Debug;
                    ref_log_when = LOG_WHEN_PROC;
                    ref_log_status = LOG_STATUS_OK;
                    log_message =
                        format!("condition expected failure exit code matched: {expectedf}");
                    failure_reason = FailureReason::Status;
                }
            }
        } else {
            match exit_status {
                // exit code is nonzero, however this might be the expected
                // behavior of the executed command: if the exit code had to be
                // checked then the check is performed with the following
                // priority rule:
                // 1. match resulting status for expected failure
                // 2. match resulting status for unsuccessfulness
                ExitStatus::Exited(v) => {
                    statusmsg = format!("ERROR/{v}");
                    severity = LogType::Debug;
                    ref_log_when = LOG_WHEN_PROC;
                    ref_log_status = LOG_STATUS_OK;
                    process_status = *v;
                    log_message = format!(
                        "command: `{command_line}` exited with FAILURE status {statusmsg}",
                    );
                    if let Some(expectedf) = failure_status {
                        if v == expectedf {
                            severity = LogType::Debug;
                            ref_log_when = LOG_WHEN_PROC;
                            ref_log_status = LOG_STATUS_OK;
                            log_message = format!(
                                "condition expected failure exit code {expectedf} matched",
                            );
                            failure_reason = FailureReason::Status;
                        } else if let Some(expected) = success_status {
                            if v == expected {
                                severity = LogType::Debug;
                                ref_log_when = LOG_WHEN_PROC;
                                ref_log_status = LOG_STATUS_OK;
                                log_message = format!(
                                    "condition expected success exit code {expected} matched",
                                );
                            } else {
                                severity = LogType::Debug;
                                ref_log_when = LOG_WHEN_PROC;
                                ref_log_status = LOG_STATUS_OK;
                                log_message = format!(
                                    "condition expected success exit code {expected} NOT matched: {v}",
                                );
                                failure_reason = FailureReason::Status;
                            }
                        } else {
                            severity = LogType::Debug;
                            ref_log_when = LOG_WHEN_PROC;
                            ref_log_status = LOG_STATUS_OK;
                            log_message = format!(
                                "condition expected failure exit code {expectedf} NOT matched",
                            );
                        }
                    } else if let Some(expected) = success_status {
                        if v == expected {
                            severity = LogType::Debug;
                            ref_log_when = LOG_WHEN_PROC;
                            ref_log_status = LOG_STATUS_OK;
                            log_message =
                                format!("condition expected success exit code {expected} matched");
                        } else {
                            severity = LogType::Debug;
                            ref_log_when = LOG_WHEN_PROC;
                            ref_log_status = LOG_STATUS_OK;
                            log_message = format!(
                                "condition expected success exit code {expected} NOT matched: {v}",
                            );
                            failure_reason = FailureReason::Status;
                        }
                    }
                    // if we are here, neither the success exit code nor the
                    // failure exit code were set by configuration, thus status
                    // is still set to NoFailure
                }
                // if the subprocess did not exit properly is considered
                // unsuccessful anyway: set the failure reason appropriately
                ExitStatus::Signaled(v) => {
                    statusmsg = format!("SIGNAL/{v}");
                    severity = LogType::Warn;
                    ref_log_when = LOG_WHEN_PROC;
                    ref_log_status = LOG_STATUS_FAIL;
                    log_message = format!("command: `{command_line}` ended for reason {statusmsg}");
                    failure_reason = FailureReason::Other;
                }
                ExitStatus::Other(v) => {
                    statusmsg = format!("UNKNOWN/{v}");
                    severity = LogType::Warn;
                    ref_log_when = LOG_WHEN_PROC;
                    ref_log_status = LOG_STATUS_FAIL;
                    log_message = format!("command: `{command_line}` ended for reason {statusmsg}");
                    failure_reason = FailureReason::Other;
                }
                ExitStatus::Undetermined => {
                    statusmsg = String::from("UNDETERMINED");
                    severity = LogType::Warn;
                    ref_log_when = LOG_WHEN_PROC;
                    ref_log_status = LOG_STATUS_FAIL;
                    log_message = format!("command: `{command_line}` ended for reason {statusmsg}");
                    failure_reason = FailureReason::Other;
                }
            }
        }

        // temporarily use the failure reason to determine whether or not to
        // check for task success in the command output
        match failure_reason {
            // only when no other failure has occurred we harvest process IO
            // and perform stdout/stderr text analysis
            FailureReason::NoFailure => {
                // command output based task result determination: both in
                // regex matching and in direct text comparison the tests are
                // performed in this order:
                //   1. against expected success in stdout
                //   2. against expected success in stderr
                //   3. against expected failure in stdout
                //   3. against expected failure in stderr
                // if any of the tests does not fail, then the further test is
                // performed; on the other side, failure in any of the tests
                // causes skipping of all the following ones

                // NOTE: in the following blocks, all the checks for
                // failure_reason not to be NoFailure are needed to bail out if
                // a failure condition has been already determined: this also
                // enforces a check priority (as described above); the first
                // of these checks is pleonastic because NoFailure has been
                // just matched, however it improves code modularity and
                // readability, and possibility to change priority by just
                // moving code: cost is small compared to this so we keep it

                // A. regular expresion checks: case sensitiveness is directly
                //    handled by the Regex engine
                if match_regexp {
                    // A.1 regex success stdout check
                    if failure_reason == FailureReason::NoFailure {
                        if let Some(p) = &success_stdout {
                            if !p.is_empty() {
                                match regex::RegexBuilder::new(p)
                                    .case_insensitive(!case_sensitive)
                                    .build()
                                {
                                    Ok(re) => {
                                        if match_exact {
                                            if re.is_match(process_stdout) {
                                                severity = LogType::Debug;
                                                ref_log_when = LOG_WHEN_PROC;
                                                ref_log_status = LOG_STATUS_OK;
                                                log_message = format!(
                                                    "condition success stdout (regex) {p:?} matched",
                                                );
                                            } else {
                                                severity = LogType::Debug;
                                                ref_log_when = LOG_WHEN_PROC;
                                                ref_log_status = LOG_STATUS_OK;
                                                log_message = format!(
                                                    "condition success stdout (regex) {p:?} NOT matched",
                                                );
                                                failure_reason = FailureReason::StdOut;
                                            }
                                        } else if re.find(process_stdout).is_some() {
                                            severity = LogType::Debug;
                                            ref_log_when = LOG_WHEN_PROC;
                                            ref_log_status = LOG_STATUS_OK;
                                            log_message = format!(
                                                "condition success stdout (regex) {p:?} found",
                                            );
                                        } else {
                                            severity = LogType::Debug;
                                            ref_log_when = LOG_WHEN_PROC;
                                            ref_log_status = LOG_STATUS_OK;
                                            log_message = format!(
                                                "condition success stdout (regex) {p:?} NOT found",
                                            );
                                            failure_reason = FailureReason::StdOut;
                                        }
                                    }
                                    _ => {
                                        severity = LogType::Error;
                                        ref_log_when = LOG_WHEN_PROC;
                                        ref_log_status = LOG_STATUS_FAIL;
                                        log_message = format!(
                                            "provided INVALID stdout regex {p:?} NOT found/matched",
                                        );
                                        failure_reason = FailureReason::StdOut;
                                    }
                                }
                            }
                        }
                    }
                    // A.2 regex success stderr check
                    if failure_reason == FailureReason::NoFailure {
                        if let Some(p) = &success_stderr {
                            if !p.is_empty() {
                                match regex::RegexBuilder::new(p)
                                    .case_insensitive(!case_sensitive)
                                    .build()
                                {
                                    Ok(re) => {
                                        if match_exact {
                                            if re.is_match(process_stderr) {
                                                severity = LogType::Debug;
                                                ref_log_when = LOG_WHEN_PROC;
                                                ref_log_status = LOG_STATUS_OK;
                                                log_message = format!(
                                                    "condition success stderr (regex) {p:?} matched",
                                                );
                                            } else {
                                                severity = LogType::Debug;
                                                ref_log_when = LOG_WHEN_PROC;
                                                ref_log_status = LOG_STATUS_OK;
                                                log_message = format!(
                                                    "condition success stderr (regex) {p:?} NOT matched",
                                                );
                                                failure_reason = FailureReason::StdErr;
                                            }
                                        } else if re.find(process_stderr).is_some() {
                                            severity = LogType::Debug;
                                            ref_log_when = LOG_WHEN_PROC;
                                            ref_log_status = LOG_STATUS_OK;
                                            log_message = format!(
                                                "condition success stderr (regex) {p:?} found",
                                            );
                                        } else {
                                            severity = LogType::Debug;
                                            ref_log_when = LOG_WHEN_PROC;
                                            ref_log_status = LOG_STATUS_OK;
                                            log_message = format!(
                                                "condition success stderr (regex) {p:?} NOT found",
                                            );
                                            failure_reason = FailureReason::StdErr;
                                        }
                                    }
                                    _ => {
                                        severity = LogType::Error;
                                        ref_log_when = LOG_WHEN_PROC;
                                        ref_log_status = LOG_STATUS_FAIL;
                                        log_message = format!(
                                            "provided INVALID stderr regex {p:?} NOT found/matched",
                                        );
                                        failure_reason = FailureReason::StdErr;
                                    }
                                }
                            }
                        }
                    }
                    // A.3 regex failure stdout check
                    if failure_reason == FailureReason::NoFailure {
                        if let Some(p) = &failure_stdout {
                            if !p.is_empty() {
                                match regex::RegexBuilder::new(p)
                                    .case_insensitive(!case_sensitive)
                                    .build()
                                {
                                    Ok(re) => {
                                        if match_exact {
                                            if re.is_match(process_stdout) {
                                                severity = LogType::Debug;
                                                ref_log_when = LOG_WHEN_PROC;
                                                ref_log_status = LOG_STATUS_OK;
                                                log_message = format!(
                                                    "condition failure stdout (regex) {p:?} matched",
                                                );
                                                failure_reason = FailureReason::StdOut;
                                            } else {
                                                severity = LogType::Debug;
                                                ref_log_when = LOG_WHEN_PROC;
                                                ref_log_status = LOG_STATUS_OK;
                                                log_message = format!(
                                                    "condition failure stdout (regex) {p:?} NOT matched",
                                                );
                                            }
                                        } else if re.find(process_stdout).is_some() {
                                            severity = LogType::Debug;
                                            ref_log_when = LOG_WHEN_PROC;
                                            ref_log_status = LOG_STATUS_OK;
                                            log_message = format!(
                                                "condition failure stdout (regex) {p:?} found",
                                            );
                                            failure_reason = FailureReason::StdOut;
                                        } else {
                                            severity = LogType::Debug;
                                            ref_log_when = LOG_WHEN_PROC;
                                            ref_log_status = LOG_STATUS_OK;
                                            log_message = format!(
                                                "condition failure stdout (regex) {p:?} NOT found",
                                            );
                                        }
                                    }
                                    _ => {
                                        severity = LogType::Error;
                                        ref_log_when = LOG_WHEN_PROC;
                                        ref_log_status = LOG_STATUS_FAIL;
                                        log_message = format!(
                                            "provided INVALID failure stdout regex {p:?} NOT found/matched",
                                        );
                                    }
                                }
                            }
                        }
                    }
                    // A.4 regex failure stderr check
                    if failure_reason == FailureReason::NoFailure {
                        if let Some(p) = &failure_stderr {
                            if !p.is_empty() {
                                match regex::RegexBuilder::new(p)
                                    .case_insensitive(!case_sensitive)
                                    .build()
                                {
                                    Ok(re) => {
                                        if match_exact {
                                            if re.is_match(process_stderr) {
                                                severity = LogType::Debug;
                                                ref_log_when = LOG_WHEN_PROC;
                                                ref_log_status = LOG_STATUS_OK;
                                                log_message = format!(
                                                    "condition success stderr (regex) {p:?} matched",
                                                );
                                                failure_reason = FailureReason::StdErr;
                                            } else {
                                                severity = LogType::Debug;
                                                ref_log_when = LOG_WHEN_PROC;
                                                ref_log_status = LOG_STATUS_OK;
                                                log_message = format!(
                                                    "condition success stderr (regex) {p:?} NOT matched",
                                                );
                                            }
                                        } else if re.find(process_stderr).is_some() {
                                            severity = LogType::Debug;
                                            ref_log_when = LOG_WHEN_PROC;
                                            ref_log_status = LOG_STATUS_OK;
                                            log_message = format!(
                                                "condition success stderr (regex) {p:?} found",
                                            );
                                            failure_reason = FailureReason::StdErr;
                                        } else {
                                            severity = LogType::Debug;
                                            ref_log_when = LOG_WHEN_PROC;
                                            ref_log_status = LOG_STATUS_OK;
                                            log_message = format!(
                                                "condition success stderr (regex) {p:?} NOT found",
                                            );
                                        }
                                    }
                                    _ => {
                                        severity = LogType::Error;
                                        ref_log_when = LOG_WHEN_PROC;
                                        ref_log_status = LOG_STATUS_FAIL;
                                        log_message = format!(
                                            "provided INVALID stderr regex {p:?} NOT found/matched",
                                        );
                                    }
                                }
                            }
                        }
                    }
                } else {
                    // B. text checks: the case sensitive and case insensitive
                    //    options are handled separately because they require
                    //    different comparisons
                    if case_sensitive {
                        // B.1a CS text success stdout check
                        if failure_reason == FailureReason::NoFailure {
                            if let Some(p) = success_stdout {
                                if !p.is_empty() {
                                    if match_exact {
                                        if process_stdout == *p {
                                            severity = LogType::Debug;
                                            ref_log_when = LOG_WHEN_PROC;
                                            ref_log_status = LOG_STATUS_OK;
                                            log_message =
                                                format!("condition success stdout {p:?} matched");
                                        } else {
                                            severity = LogType::Debug;
                                            ref_log_when = LOG_WHEN_PROC;
                                            ref_log_status = LOG_STATUS_OK;
                                            log_message = format!(
                                                "condition success stdout {p:?} NOT matched",
                                            );
                                            failure_reason = FailureReason::StdOut;
                                        }
                                    } else if process_stdout.contains(p) {
                                        severity = LogType::Debug;
                                        ref_log_when = LOG_WHEN_PROC;
                                        ref_log_status = LOG_STATUS_OK;
                                        log_message =
                                            format!("condition success stdout {p:?} found");
                                    } else {
                                        severity = LogType::Debug;
                                        ref_log_when = LOG_WHEN_PROC;
                                        ref_log_status = LOG_STATUS_OK;
                                        log_message =
                                            format!("condition success stdout {p:?} NOT found");
                                        failure_reason = FailureReason::StdOut;
                                    }
                                }
                            }
                        }
                        // B.2a CS text success stderr check
                        if failure_reason == FailureReason::NoFailure {
                            if let Some(p) = success_stderr {
                                if !p.is_empty() {
                                    if match_exact {
                                        if process_stderr == *p {
                                            severity = LogType::Debug;
                                            ref_log_when = LOG_WHEN_PROC;
                                            ref_log_status = LOG_STATUS_OK;
                                            log_message =
                                                format!("condition success stderr {p:?} matched");
                                        } else {
                                            severity = LogType::Debug;
                                            ref_log_when = LOG_WHEN_PROC;
                                            ref_log_status = LOG_STATUS_OK;
                                            log_message = format!(
                                                "condition success stderr {p:?} NOT matched",
                                            );
                                            failure_reason = FailureReason::StdErr;
                                        }
                                    } else if process_stderr.contains(p) {
                                        severity = LogType::Debug;
                                        ref_log_when = LOG_WHEN_PROC;
                                        ref_log_status = LOG_STATUS_OK;
                                        log_message =
                                            format!("condition success stderr {p:?} found");
                                    } else {
                                        severity = LogType::Debug;
                                        ref_log_when = LOG_WHEN_PROC;
                                        ref_log_status = LOG_STATUS_OK;
                                        log_message =
                                            format!("condition success stderr {p:?} NOT found");
                                        failure_reason = FailureReason::StdErr;
                                    }
                                }
                            }
                        }
                        // B.3a CS text failure stdout check
                        if failure_reason == FailureReason::NoFailure {
                            if let Some(p) = failure_stdout {
                                if !p.is_empty() {
                                    if match_exact {
                                        if process_stdout == *p {
                                            severity = LogType::Debug;
                                            ref_log_when = LOG_WHEN_PROC;
                                            ref_log_status = LOG_STATUS_OK;
                                            log_message =
                                                format!("condition failure stdout {p:?} matched");
                                            failure_reason = FailureReason::StdOut;
                                        } else {
                                            severity = LogType::Debug;
                                            ref_log_when = LOG_WHEN_PROC;
                                            ref_log_status = LOG_STATUS_OK;
                                            log_message = format!(
                                                "condition failure stdout {p:?} NOT matched",
                                            );
                                        }
                                    } else if process_stdout.contains(p) {
                                        severity = LogType::Debug;
                                        ref_log_when = LOG_WHEN_PROC;
                                        ref_log_status = LOG_STATUS_OK;
                                        log_message =
                                            format!("condition failure stdout {p:?} found");
                                        failure_reason = FailureReason::StdOut;
                                    } else {
                                        severity = LogType::Debug;
                                        ref_log_when = LOG_WHEN_PROC;
                                        ref_log_status = LOG_STATUS_OK;
                                        log_message =
                                            format!("condition failure stdout {p:?} NOT found");
                                    }
                                }
                            }
                        }
                        // B.4a CS text failure stderr check
                        if failure_reason == FailureReason::NoFailure {
                            if let Some(p) = failure_stderr {
                                if !p.is_empty() {
                                    if match_exact {
                                        if process_stderr == *p {
                                            severity = LogType::Debug;
                                            ref_log_when = LOG_WHEN_PROC;
                                            ref_log_status = LOG_STATUS_OK;
                                            log_message =
                                                format!("condition failure stderr {p:?} matched");
                                            failure_reason = FailureReason::StdErr;
                                        } else {
                                            severity = LogType::Debug;
                                            ref_log_when = LOG_WHEN_PROC;
                                            ref_log_status = LOG_STATUS_OK;
                                            log_message = format!(
                                                "condition failure stderr {p:?} NOT matched",
                                            );
                                        }
                                    } else if process_stderr.contains(p) {
                                        severity = LogType::Debug;
                                        ref_log_when = LOG_WHEN_PROC;
                                        ref_log_status = LOG_STATUS_OK;
                                        log_message =
                                            format!("condition failure stderr {p:?} found");
                                        failure_reason = FailureReason::StdErr;
                                    } else {
                                        severity = LogType::Debug;
                                        ref_log_when = LOG_WHEN_PROC;
                                        ref_log_status = LOG_STATUS_OK;
                                        log_message =
                                            format!("condition failure stderr {p:?} NOT found");
                                    }
                                }
                            }
                        }
                    } else {
                        // B.1b CI text success stdout check
                        if failure_reason == FailureReason::NoFailure {
                            if let Some(p) = success_stdout {
                                if !p.is_empty() {
                                    if match_exact {
                                        if process_stdout.to_uppercase() == p.to_uppercase() {
                                            severity = LogType::Debug;
                                            ref_log_when = LOG_WHEN_PROC;
                                            ref_log_status = LOG_STATUS_OK;
                                            log_message =
                                                format!("condition success stdout {p:?} matched");
                                        } else {
                                            severity = LogType::Debug;
                                            ref_log_when = LOG_WHEN_PROC;
                                            ref_log_status = LOG_STATUS_OK;
                                            log_message = format!(
                                                "condition success stdout {p:?} NOT matched",
                                            );
                                            failure_reason = FailureReason::StdOut;
                                        }
                                    } else if process_stdout
                                        .to_uppercase()
                                        .contains(&p.to_uppercase())
                                    {
                                        severity = LogType::Debug;
                                        ref_log_when = LOG_WHEN_PROC;
                                        ref_log_status = LOG_STATUS_OK;
                                        log_message =
                                            format!("condition success stdout {p:?} found");
                                    } else {
                                        severity = LogType::Debug;
                                        ref_log_when = LOG_WHEN_PROC;
                                        ref_log_status = LOG_STATUS_OK;
                                        log_message =
                                            format!("condition success stdout {p:?} NOT found");
                                        failure_reason = FailureReason::StdOut;
                                    }
                                }
                            }
                        }
                        // B.2b CI text success stderr check
                        if failure_reason == FailureReason::NoFailure {
                            if let Some(p) = success_stderr {
                                if !p.is_empty() {
                                    if match_exact {
                                        if process_stderr.to_uppercase() == p.to_uppercase() {
                                            severity = LogType::Debug;
                                            ref_log_when = LOG_WHEN_PROC;
                                            ref_log_status = LOG_STATUS_OK;
                                            log_message =
                                                format!("condition success stderr {p:?} matched");
                                        } else {
                                            severity = LogType::Debug;
                                            ref_log_when = LOG_WHEN_PROC;
                                            ref_log_status = LOG_STATUS_OK;
                                            log_message = format!(
                                                "condition success stderr {p:?} NOT matched",
                                            );
                                            failure_reason = FailureReason::StdErr;
                                        }
                                    } else if process_stderr
                                        .to_uppercase()
                                        .contains(&p.to_uppercase())
                                    {
                                        severity = LogType::Debug;
                                        ref_log_when = LOG_WHEN_PROC;
                                        ref_log_status = LOG_STATUS_OK;
                                        log_message =
                                            format!("condition success stderr {p:?} found");
                                    } else {
                                        severity = LogType::Debug;
                                        ref_log_when = LOG_WHEN_PROC;
                                        ref_log_status = LOG_STATUS_OK;
                                        log_message =
                                            format!("condition success stderr {p:?} NOT found");
                                        failure_reason = FailureReason::StdErr;
                                    }
                                }
                            }
                        }
                        // B.3b CI text failure stdout check
                        if failure_reason == FailureReason::NoFailure {
                            if let Some(p) = failure_stdout {
                                if !p.is_empty() {
                                    if match_exact {
                                        if process_stdout.to_uppercase() == p.to_uppercase() {
                                            severity = LogType::Debug;
                                            ref_log_when = LOG_WHEN_PROC;
                                            ref_log_status = LOG_STATUS_OK;
                                            log_message =
                                                format!("condition failure stdout {p:?} matched");
                                            failure_reason = FailureReason::StdOut;
                                        } else {
                                            severity = LogType::Debug;
                                            ref_log_when = LOG_WHEN_PROC;
                                            ref_log_status = LOG_STATUS_OK;
                                            log_message = format!(
                                                "condition failure stdout {p:?} NOT matched",
                                            );
                                        }
                                    } else if process_stdout
                                        .to_uppercase()
                                        .contains(&p.to_uppercase())
                                    {
                                        severity = LogType::Debug;
                                        ref_log_when = LOG_WHEN_PROC;
                                        ref_log_status = LOG_STATUS_OK;
                                        log_message =
                                            format!("condition failure stdout {p:?} found");
                                        failure_reason = FailureReason::StdOut;
                                    } else {
                                        severity = LogType::Debug;
                                        ref_log_when = LOG_WHEN_PROC;
                                        ref_log_status = LOG_STATUS_OK;
                                        log_message =
                                            format!("condition failure stdout {p:?} NOT found");
                                    }
                                }
                            }
                        }
                        // B.4b CI text failure stderr check
                        if failure_reason == FailureReason::NoFailure {
                            if let Some(p) = failure_stderr {
                                if !p.is_empty() {
                                    if match_exact {
                                        if process_stderr.to_uppercase() == p.to_uppercase() {
                                            severity = LogType::Debug;
                                            ref_log_when = LOG_WHEN_PROC;
                                            ref_log_status = LOG_STATUS_OK;
                                            log_message =
                                                format!("condition failure stderr {p:?} matched");
                                            failure_reason = FailureReason::StdErr;
                                        } else {
                                            severity = LogType::Debug;
                                            ref_log_when = LOG_WHEN_PROC;
                                            ref_log_status = LOG_STATUS_OK;
                                            log_message = format!(
                                                "condition failure stderr {p:?} NOT matched",
                                            );
                                        }
                                    } else if process_stderr
                                        .to_uppercase()
                                        .contains(&p.to_uppercase())
                                    {
                                        severity = LogType::Debug;
                                        ref_log_when = LOG_WHEN_PROC;
                                        ref_log_status = LOG_STATUS_OK;
                                        log_message =
                                            format!("condition failure stderr {p:?} found");
                                        failure_reason = FailureReason::StdErr;
                                    } else {
                                        severity = LogType::Debug;
                                        ref_log_when = LOG_WHEN_PROC;
                                        ref_log_status = LOG_STATUS_OK;
                                        log_message =
                                            format!("condition failure stderr {p:?} NOT found");
                                    }
                                }
                            }
                        }
                    }
                }
            }
            _ => {
                // need not to check for other failure types
                process_failed = true;
            }
        }

        // returns this:
        (
            process_status,
            process_failed,
            failure_reason,
            severity,
            ref_log_when,
            ref_log_status,
            log_message,
        )
    }
}

#[allow(dead_code)]
/// This module provides utilities for Lua based items
pub mod luaitem {

    /// The possible values to be checked from Lua
    #[derive(Debug)]
    pub enum LuaValue {
        LuaString(String),
        LuaNumber(f64),
        LuaBoolean(bool),
    }

    /// In case of failure, the reason will be one of the provided values
    #[derive(Debug, PartialEq)]
    pub enum FailureReason {
        NoCheck,
        NoFailure,
        VariableMatch,
        ScriptError,
        InitError,
    }
}

#[cfg(feature = "dbus")]
#[allow(dead_code)]
pub mod dbusitem {
    use crate::LogType;
    use crate::constants::*;
    use cfgmap::CfgValue;
    use regex::Regex;
    use std::collections::HashMap;
    use std::hash::{Hash, Hasher};
    use zbus;
    use zbus::Message;
    use zbus::zvariant;
    use zbus::zvariant::Signature;

    /// an enum to store the operators for checking signal parameters
    #[derive(PartialEq, Hash, Clone, Debug)]
    pub enum ParamCheckOperator {
        Equal,        // "eq"
        NotEqual,     // "neq"
        Greater,      // "gt"
        GreaterEqual, // "ge"
        Less,         // "lt"
        LessEqual,    // "le"
        Match,        // "match"
        Contains,     // "contains"
        NotContains,  // "ncontains"
    }

    /// an enum containing the value that the parameter should be checked
    /// against
    #[derive(Debug)]
    pub enum ParameterCheckValue {
        Boolean(bool),
        Integer(i64),
        Float(f64),
        String(String),
        Regex(Regex),
    }

    /// an enum containing the possible types of indexes for parameters
    #[derive(Hash, Debug)]
    pub enum ParameterIndex {
        Integer(u64),
        String(String),
    }

    /// a struct containing a single test to be performed against a signal
    /// payload
    ///
    /// short explaination, so that I remember how to use it:
    /// - `Index`: contains a list of indexes which specify, also for nested
    ///   structures. This means that for an array of mappings it might be of
    ///   the form `{ 1, 3, "somepos" }` where the first`1` is the argument
    ///   index, the `3` is the array index, and `"somepos"` is the mapping
    ///   index.
    /// - `Operator`: the operator to check the payload against
    /// - `Value`: the value to compare the parameter entry to
    #[derive(Debug)]
    pub struct ParameterCheckTest {
        pub index: Vec<ParameterIndex>,
        pub operator: ParamCheckOperator,
        pub value: ParameterCheckValue,
    }

    // implement the hash protocol for ParameterCheckTest
    impl Hash for ParameterCheckTest {
        fn hash<H: Hasher>(&self, state: &mut H) {
            self.index.hash(state);
            self.operator.hash(state);
            match &self.value {
                ParameterCheckValue::Boolean(x) => x.hash(state),
                ParameterCheckValue::Integer(x) => x.hash(state),
                ParameterCheckValue::Float(x) => x.to_bits().hash(state),
                ParameterCheckValue::String(x) => x.hash(state),
                ParameterCheckValue::Regex(x) => x.as_str().hash(state),
            }
        }
    }

    // allow a test to be easily cloned
    impl Clone for ParameterCheckTest {
        fn clone(&self) -> Self {
            let mut index: Vec<ParameterIndex> = Vec::new();
            for i in self.index.iter() {
                index.push({
                    match i {
                        ParameterIndex::Integer(u) => ParameterIndex::Integer(*u),
                        ParameterIndex::String(s) => ParameterIndex::String(s.clone()),
                    }
                });
            }
            let value = match &self.value {
                ParameterCheckValue::Boolean(x) => ParameterCheckValue::Boolean(*x),
                ParameterCheckValue::Integer(x) => ParameterCheckValue::Integer(*x),
                ParameterCheckValue::Float(x) => ParameterCheckValue::Float(*x),
                ParameterCheckValue::String(s) => ParameterCheckValue::String(s.clone()),
                ParameterCheckValue::Regex(e) => ParameterCheckValue::Regex(e.clone()),
            };

            ParameterCheckTest {
                index,
                operator: self.operator.clone(),
                value,
            }
        }
    }

    /// a trait that defines containable types: implementations are provided for
    /// all types found in the `ParameterCheckValue` enum defined above
    pub trait Containable {
        fn is_contained_in(&self, v: &zvariant::Value) -> bool;
    }

    // implementations: dictionary value lookup will be provided as soon as there
    // will be a way, in _zbus_, to at least retrieve the dictionary keys (if not
    // directly the mapped values) in order to compare the contents with the value
    impl Containable for bool {
        fn is_contained_in(&self, v: &zvariant::Value) -> bool {
            match v {
                zvariant::Value::Array(a) => a.contains(&zvariant::Value::from(self)),
                _ => false,
            }
        }
    }

    impl Containable for i64 {
        fn is_contained_in(&self, v: &zvariant::Value) -> bool {
            match v {
                zvariant::Value::Array(a) => {
                    // to handle this we transform the array into a new array of
                    // i64 that is created to test for inclusion, and large u64
                    // numbers are be automatically discarded and set to `None`
                    // which is never matched
                    let testv: Vec<Option<i64>> = match a.element_signature() {
                        Signature::U8 => {
                            // BYTE
                            a.iter()
                                .map(|x| {
                                    if let zvariant::Value::U8(z) = x {
                                        Some(i64::from(*z))
                                    } else {
                                        None
                                    }
                                })
                                .collect()
                        }
                        Signature::I16 => {
                            // INT16
                            a.iter()
                                .map(|x| {
                                    if let zvariant::Value::I16(z) = x {
                                        Some(i64::from(*z))
                                    } else {
                                        None
                                    }
                                })
                                .collect()
                        }
                        Signature::U16 => {
                            // UINT16
                            a.iter()
                                .map(|x| {
                                    if let zvariant::Value::I16(z) = x {
                                        Some(i64::from(*z))
                                    } else {
                                        None
                                    }
                                })
                                .collect()
                        }
                        Signature::I32 => {
                            // INT32
                            a.iter()
                                .map(|x| {
                                    if let zvariant::Value::I32(z) = x {
                                        Some(i64::from(*z))
                                    } else {
                                        None
                                    }
                                })
                                .collect()
                        }
                        Signature::U32 => {
                            // UINT32
                            a.iter()
                                .map(|x| {
                                    if let zvariant::Value::U32(z) = x {
                                        Some(i64::from(*z))
                                    } else {
                                        None
                                    }
                                })
                                .collect()
                        }
                        Signature::I64 => {
                            // INT64
                            a.iter()
                                .map(|x| {
                                    if let zvariant::Value::I64(z) = x {
                                        Some(*z)
                                    } else {
                                        None
                                    }
                                })
                                .collect()
                        }
                        Signature::U64 => {
                            // UINT64
                            // this is the tricky one, but since we know that big
                            // unsigned integer surely do not match the provided
                            // value, we just convert them to `None` here, which
                            // will never match
                            a.iter()
                                .map(|x| {
                                    if let zvariant::Value::U64(z) = x {
                                        if *z <= i64::MAX as u64 {
                                            Some(*z as i64)
                                        } else {
                                            None
                                        }
                                    } else {
                                        None
                                    }
                                })
                                .collect()
                        }
                        _ => {
                            return false;
                        }
                    };
                    testv.contains(&Some(*self))
                }
                _ => false,
            }
        }
    }

    impl Containable for f64 {
        fn is_contained_in(&self, v: &zvariant::Value) -> bool {
            match v {
                zvariant::Value::Array(a) => a.contains(&zvariant::Value::from(*self)),
                _ => false,
            }
        }
    }

    // String is a particular case, because it has to look for presence in arrays
    // (both of `Str` and `ObjectPath`) or, alternatively, to match a substring
    // of the returned `Str` or `ObjectPath`
    impl Containable for String {
        fn is_contained_in(&self, v: &zvariant::Value) -> bool {
            match v {
                zvariant::Value::Str(s) => s.as_str().contains(self.as_str()),
                zvariant::Value::ObjectPath(s) => s.as_str().contains(self.as_str()),
                zvariant::Value::Array(a) => match a.element_signature() {
                    Signature::Str => a.contains(&zvariant::Value::from(self)),
                    Signature::ObjectPath => {
                        let o = zvariant::ObjectPath::try_from(self.as_str());
                        if let Ok(o) = o {
                            a.contains(&zvariant::Value::from(o))
                        } else {
                            false
                        }
                    }
                    _ => false,
                },
                // currently used version of Dict (the one in zbus 3.x) does not
                // allow to search the keys as set or list, thus the easiest test
                // that can be made is retrieving a value and checking for errors
                // and that the result is something
                // !!!! zvariant::Value::Dict(d) => match key_signature(d).as_str() {
                zvariant::Value::Dict(d) => {
                    if let Signature::Dict { key: ks, value: _ } = d.signature() {
                        match ks.signature() {
                            Signature::Str => {
                                let k = zvariant::Str::from(self.as_str());
                                let res: Result<Option<&zvariant::Value>, zvariant::Error> =
                                    d.get(&k);
                                if let Ok(res) = res {
                                    res.is_some()
                                } else {
                                    false
                                }
                            }
                            Signature::ObjectPath => {
                                let k = zvariant::ObjectPath::try_from(self.as_str());
                                if let Ok(k) = k {
                                    let res: Result<
                                        Option<zbus::zvariant::ObjectPath<'_>>,
                                        zvariant::Error,
                                    > = d.get(&k);
                                    if let Ok(res) = res {
                                        res.is_some()
                                    } else {
                                        false
                                    }
                                } else {
                                    false
                                }
                            }
                            _ => false,
                        }
                    } else {
                        false
                    }
                }
                _ => false,
            }
        }
    }

    // the following is totally arbitrary and will actually not be used: it is
    // provided here only in order to complete the "required" implementations
    impl Containable for Regex {
        fn is_contained_in(&self, v: &zvariant::Value) -> bool {
            match v {
                zvariant::Value::Array(a) => {
                    for elem in a.iter().cloned() {
                        if let zvariant::Value::Str(s) = elem {
                            if self.is_match(s.as_str()) {
                                return true;
                            }
                        }
                    }
                    false
                }
                _ => false,
            }
        }
    }

    // the trait used to convert values to `zvariant::Value`
    pub trait ToVariant {
        fn to_variant(&self) -> Option<zvariant::Value<'_>>;
    }

    impl ToVariant for bool {
        fn to_variant(&self) -> Option<zvariant::Value<'_>> {
            Some(zvariant::Value::Bool(*self))
        }
    }

    impl ToVariant for i64 {
        fn to_variant(&self) -> Option<zvariant::Value<'_>> {
            Some(zvariant::Value::I64(*self))
        }
    }

    impl ToVariant for f64 {
        fn to_variant(&self) -> Option<zvariant::Value<'_>> {
            Some(zvariant::Value::F64(*self))
        }
    }

    impl ToVariant for str {
        fn to_variant(&self) -> Option<zvariant::Value<'_>> {
            let s = &self.to_string();
            if s.starts_with('\\') {
                let rest = s.clone().split_off(2);
                if s.starts_with("\\b") {
                    let rest = rest.to_lowercase();
                    if rest == "true" || rest == "1" {
                        Some(zvariant::Value::Bool(true))
                    } else if rest == "false" || rest == "0" {
                        Some(zvariant::Value::Bool(false))
                    } else {
                        None
                    }
                } else if s.starts_with("\\y") {
                    if let Ok(v) = rest.parse::<u8>() {
                        Some(zvariant::Value::U8(v))
                    } else {
                        None
                    }
                } else if s.starts_with("\\n") {
                    if let Ok(v) = rest.parse::<i16>() {
                        Some(zvariant::Value::I16(v))
                    } else {
                        None
                    }
                } else if s.starts_with("\\q") {
                    if let Ok(v) = rest.parse::<u16>() {
                        Some(zvariant::Value::U16(v))
                    } else {
                        None
                    }
                } else if s.starts_with("\\i") {
                    if let Ok(v) = rest.parse::<i32>() {
                        Some(zvariant::Value::I32(v))
                    } else {
                        None
                    }
                } else if s.starts_with("\\u") {
                    if let Ok(v) = rest.parse::<u32>() {
                        Some(zvariant::Value::U32(v))
                    } else {
                        None
                    }
                } else if s.starts_with("\\x") {
                    if let Ok(v) = rest.parse::<i64>() {
                        Some(zvariant::Value::I64(v))
                    } else {
                        None
                    }
                } else if s.starts_with("\\t") {
                    if let Ok(v) = rest.parse::<u64>() {
                        Some(zvariant::Value::U64(v))
                    } else {
                        None
                    }
                } else if s.starts_with("\\d") {
                    if let Ok(v) = rest.parse::<f64>() {
                        Some(zvariant::Value::F64(v))
                    } else {
                        None
                    }
                } else if s.starts_with("\\s") {
                    Some(zvariant::Value::new(rest.clone()))
                } else if s.starts_with("\\o") {
                    // here we check it, having the RE at hand
                    if RE_DBUS_OBJECT_PATH.is_match(&rest) {
                        Some(zvariant::Value::new(
                            zvariant::ObjectPath::try_from(rest.clone()).unwrap(),
                        ))
                    } else {
                        None
                    }
                } else if s.starts_with("\\\\") {
                    Some(zvariant::Value::new(String::from("\\") + &rest))
                } else {
                    Some(zvariant::Value::new(String::from(s)))
                }
            } else {
                Some(zvariant::Value::new(String::from(s)))
            }
        }
    }

    impl<T> ToVariant for Vec<T>
    where
        T: ToVariant,
    {
        fn to_variant(&self) -> Option<zvariant::Value<'_>> {
            let mut a: Vec<zvariant::Value> = Vec::new();
            for item in self.iter() {
                if let Some(v) = item.to_variant() {
                    a.push(v)
                } else {
                    return None;
                }
            }
            Some(zvariant::Value::new(a))
        }
    }

    // we only support maps where the key is a string
    impl<T> ToVariant for HashMap<String, T>
    where
        T: ToVariant,
    {
        fn to_variant(&self) -> Option<zvariant::Value<'_>> {
            let mut d: HashMap<String, zvariant::Value> = HashMap::new();
            for (key, item) in self.iter() {
                if let Some(v) = item.to_variant() {
                    d.insert(key.clone(), v);
                } else {
                    return None;
                }
            }
            Some(zvariant::Value::new(d))
        }
    }

    // this is necessary for the following conversion
    impl ToVariant for zvariant::Value<'_> {
        fn to_variant(&self) -> Option<zvariant::Value<'_>> {
            Some(self.clone())
        }
    }

    // and finally we support CfgValue, which is similar to a variant
    impl ToVariant for CfgValue {
        fn to_variant(&self) -> Option<zvariant::Value<'_>> {
            if self.is_bool() {
                self.as_bool().unwrap().to_variant()
            } else if self.is_int() {
                self.as_int().unwrap().to_variant()
            } else if self.is_float() {
                self.as_float().unwrap().to_variant()
            } else if self.is_str() {
                self.as_str().unwrap().to_variant()
            } else if self.is_list() {
                self.as_list().unwrap().to_variant()
            } else if self.is_map() {
                let map = self.as_map().unwrap();
                let mut h: HashMap<String, zvariant::Value> = HashMap::new();
                for key in map.keys() {
                    if let Some(value) = map.get(key) {
                        if let Some(v) = value.to_variant() {
                            h.insert(key.clone(), v);
                        } else {
                            return None;
                        }
                    } else {
                        return None;
                    }
                }
                Some(zvariant::Value::new(h))
            } else {
                None
            }
        }
    }

    // a helper to apply a given operator to two values without clutter;
    // for simplicity sake the `Match` operator will just evaluate to
    // `false` here, instead of generating an error: the `Err()` case would
    // clutter the code for numerical comparisons uslessly, as we also know
    // that the test are built only via `load_cfgmap`, and that it only
    // admits 'match' for regular expressions; the `Contains` operator also
    // evaluates to `false` here since this function only compares args
    // that are `PartialOrd+PartialEq`, and arrays are not
    fn _oper<T: PartialOrd + PartialEq>(op: &ParamCheckOperator, o1: T, o2: T) -> bool {
        match op {
            ParamCheckOperator::Equal => o1 == o2,
            ParamCheckOperator::NotEqual => o1 != o2,
            ParamCheckOperator::Less => o1 < o2,
            ParamCheckOperator::LessEqual => o1 <= o2,
            ParamCheckOperator::Greater => o1 > o2,
            ParamCheckOperator::GreaterEqual => o1 >= o2,
            ParamCheckOperator::Match => false,
            ParamCheckOperator::Contains => false,
            ParamCheckOperator::NotContains => false,
        }
    }

    // the following function allows for better readability
    fn _contained_in<T: Containable>(v: &T, a: &zvariant::Value) -> bool {
        v.is_contained_in(a)
    }

    /// This is the heart of DBus message/parameter checks:it takes references
    /// to the message and to the list of checks that has been configured, and
    /// performs the checks (all or some depends on the value of `checks_all`)
    /// on the message contents.
    ///
    /// It returns a boolean that specifies if the check was successful or not
    /// (under the condition that either some or all the parameters had to be
    /// tested), and four other values that form the variable part of a log
    /// message, suitable for being issued by the specialized logging function
    /// defined by the item.
    pub fn dbus_check_message(
        message: &Message,             // the dbus message
        checks: &[ParameterCheckTest], // item.param_checks
        checks_all: bool,              // item.checks_all
    ) -> (
        bool,         // verified
        LogType,      // the log severity
        &'static str, // log/when (LOG_WHEN_...)
        &'static str, // log/status (LOG_STATUS_...)
        String,       // log message
    ) {
        let mut verified: bool = checks_all;
        let mut severity: LogType = LogType::Trace;
        let mut ref_log_when: &str = LOG_WHEN_PROC;
        let mut ref_log_status: &str = LOG_STATUS_OK;
        let mut log_message: String = String::from("message or return parameter check ended");

        // !!!! if let Ok(mbody) = message.body() {
        let b = message.body();
        let bs = b.deserialize::<zvariant::Structure>();
        if let Ok(mbody) = bs {
            // the label is set to make sure that we can break out from
            // any nested loop on shortcut evaluation condition (that is
            // when all condition had to be true and at least one is false
            // or when one true condition is sufficient and we find it)
            // or when an error occurs, which implies evaluation to false
            let mbody = mbody.fields();
            'params: for ck in checks.iter() {
                let argnum = ck.index.first();
                if let Some(argnum) = argnum {
                    match argnum {
                        ParameterIndex::Integer(x) => {
                            if *x >= mbody.len() as u64 {
                                severity = LogType::Warn;
                                ref_log_when = LOG_WHEN_PROC;
                                ref_log_status = LOG_STATUS_FAIL;
                                log_message =
                                    format!("could not retrieve result: index {x} out of range");
                                verified = false;
                                break 'params;
                            }
                            let s = mbody.get(*x as usize);
                            if s.is_none() {
                                severity = LogType::Warn;
                                ref_log_when = LOG_WHEN_PROC;
                                ref_log_status = LOG_STATUS_FAIL;
                                log_message = format!(
                                    "could not retrieve result: index {x} provided no value"
                                );
                                verified = false;
                                break 'params;
                            }
                            let mut field_value = s.unwrap();
                            for x in 1..ck.index.len() {
                                match ck.index.get(x).unwrap() {
                                    ParameterIndex::Integer(i) => {
                                        let i = *i as usize;
                                        match field_value {
                                            zvariant::Value::Array(f) => {
                                                if i >= f.len() {
                                                    severity = LogType::Warn;
                                                    ref_log_when = LOG_WHEN_PROC;
                                                    ref_log_status = LOG_STATUS_FAIL;
                                                    log_message = format!(
                                                        "could not retrieve result: index {i} out of range",
                                                    );
                                                    verified = false;
                                                    break 'params;
                                                }
                                                // if something is wrong here, either the test
                                                // or the next "parameter shift" will go berserk
                                                field_value = &f[i];
                                            }
                                            zvariant::Value::Structure(f) => {
                                                let f = f.fields();
                                                if i >= f.len() {
                                                    severity = LogType::Warn;
                                                    ref_log_when = LOG_WHEN_PROC;
                                                    ref_log_status = LOG_STATUS_FAIL;
                                                    log_message = format!(
                                                        "could not retrieve result: index {i} out of range",
                                                    );
                                                    verified = false;
                                                    break 'params;
                                                }
                                                if let Some(v) = f.get(i) {
                                                    field_value = v;
                                                } else {
                                                    severity = LogType::Warn;
                                                    ref_log_when = LOG_WHEN_PROC;
                                                    ref_log_status = LOG_STATUS_FAIL;
                                                    log_message = format!(
                                                        "could not retrieve result: index {i} provided no value",
                                                    );
                                                    verified = false;
                                                    break 'params;
                                                }
                                            }
                                            _ => {
                                                severity = LogType::Warn;
                                                ref_log_when = LOG_WHEN_PROC;
                                                ref_log_status = LOG_STATUS_FAIL;
                                                log_message = format!(
                                                    "could not retrieve result using index {i}",
                                                );
                                                verified = false;
                                                break 'params;
                                            }
                                        }
                                    }
                                    // even though there would be the possibility to explicitly indicate an ObjectPath
                                    // via an '\o' prefix (as we do for values), since object paths really are strings
                                    // that adhere to a certain format, the conversion is automatically done for the
                                    // cases where an ObjectPath is required in place of a string - just logging that
                                    // a malformed string was configured as index where a well-formed object path had
                                    // to be provided; the following code directly matching the signature beginning is
                                    // in fact ugly as hell, however a nicer implementation might come with more recent
                                    // releases of zbus, which provide enum variants for signature nested structures
                                    ParameterIndex::String(s) => {
                                        // let s = s.as_str();
                                        match field_value {
                                            zvariant::Value::Dict(f) => {
                                                // in order to match either strings or object paths, match key signature
                                                let m = if let Signature::Dict {
                                                    key: ks,
                                                    value: _,
                                                } = f.signature()
                                                {
                                                    match **ks {
                                                        Signature::Str => f.get(s),
                                                        Signature::ObjectPath => {
                                                            if zvariant::ObjectPath::try_from(
                                                                s.as_str(),
                                                            )
                                                            .is_err()
                                                            {
                                                                severity = LogType::Warn;
                                                                ref_log_when = LOG_WHEN_PROC;
                                                                ref_log_status = LOG_STATUS_FAIL;
                                                                log_message = format!(
                                                                    "could not retrieve result: index `{s}` should be an object path",
                                                                );
                                                                verified = false;
                                                                break 'params;
                                                            } else {
                                                                f.get(s)
                                                            }
                                                        }
                                                        _ => {
                                                            severity = LogType::Warn;
                                                            ref_log_when = LOG_WHEN_PROC;
                                                            ref_log_status = LOG_STATUS_FAIL;
                                                            log_message = format!(
                                                                "could not retrieve result: index `{s}` not matching dictionary key type",
                                                            );
                                                            verified = false;
                                                            break 'params;
                                                        }
                                                    }
                                                } else {
                                                    severity = LogType::Warn;
                                                    ref_log_when = LOG_WHEN_PROC;
                                                    ref_log_status = LOG_STATUS_FAIL;
                                                    log_message = format!(
                                                        "could not retrieve result: index `{s}` not matching dictionary key type",
                                                    );
                                                    verified = false;
                                                    break 'params;
                                                };
                                                field_value = match m {
                                                    Ok(fv) => {
                                                        if let Some(fv) = fv {
                                                            fv
                                                        } else {
                                                            severity = LogType::Warn;
                                                            ref_log_when = LOG_WHEN_PROC;
                                                            ref_log_status = LOG_STATUS_FAIL;
                                                            log_message = format!(
                                                                "could not retrieve result: index `{s}` invalid",
                                                            );
                                                            verified = false;
                                                            break 'params;
                                                        }
                                                    }
                                                    Err(_) => {
                                                        severity = LogType::Warn;
                                                        ref_log_when = LOG_WHEN_PROC;
                                                        ref_log_status = LOG_STATUS_FAIL;
                                                        log_message = format!(
                                                            "could not retrieve result using index `{s}`",
                                                        );
                                                        verified = false;
                                                        break 'params;
                                                    }
                                                }
                                            }
                                            _ => {
                                                severity = LogType::Warn;
                                                ref_log_when = LOG_WHEN_PROC;
                                                ref_log_status = LOG_STATUS_FAIL;
                                                log_message = format!(
                                                    "could not retrieve parameter using index `{s}`",
                                                );
                                                verified = false;
                                                break 'params;
                                            }
                                        }
                                    }
                                }
                            }

                            // if the result is still encapsulated in a Value, take it out
                            while let zvariant::Value::Value(v) = field_value {
                                field_value = v;
                            }

                            // now we should be ready for actual testing
                            match &ck.value {
                                ParameterCheckValue::Boolean(b) => {
                                    if ck.operator == ParamCheckOperator::Equal {
                                        match field_value {
                                            zvariant::Value::Bool(v) => {
                                                if *b == *v {
                                                    verified = true;
                                                    if !checks_all {
                                                        break 'params;
                                                    }
                                                } else {
                                                    verified = false;
                                                    if checks_all {
                                                        break 'params;
                                                    }
                                                }
                                            }
                                            e => {
                                                severity = LogType::Warn;
                                                ref_log_when = LOG_WHEN_PROC;
                                                ref_log_status = LOG_STATUS_FAIL;
                                                log_message = format!(
                                                    "mismatched result type: boolean expected (got `{e:?}`)",
                                                );
                                                verified = false;
                                                break;
                                            }
                                        }
                                    } else if ck.operator == ParamCheckOperator::Contains {
                                        match field_value {
                                            zvariant::Value::Array(_) => {
                                                if _contained_in(b, field_value) {
                                                    verified = true;
                                                    if !checks_all {
                                                        break 'params;
                                                    }
                                                } else {
                                                    verified = false;
                                                    if checks_all {
                                                        break 'params;
                                                    }
                                                }
                                            }
                                            _ => {
                                                verified = false;
                                                break;
                                            }
                                        }
                                    } else if ck.operator == ParamCheckOperator::NotContains {
                                        match field_value {
                                            zvariant::Value::Array(_) => {
                                                if !_contained_in(b, field_value) {
                                                    verified = true;
                                                    if !checks_all {
                                                        break 'params;
                                                    }
                                                } else {
                                                    verified = false;
                                                    if checks_all {
                                                        break 'params;
                                                    }
                                                }
                                            }
                                            // incompatible checks should always yield false
                                            _ => {
                                                verified = false;
                                                break;
                                            }
                                        }
                                    } else {
                                        severity = LogType::Warn;
                                        ref_log_when = LOG_WHEN_PROC;
                                        ref_log_status = LOG_STATUS_FAIL;
                                        log_message = String::from("invalid operator for boolean");
                                        verified = false;
                                        break;
                                    }
                                }
                                ParameterCheckValue::Integer(i) => match field_value {
                                    zvariant::Value::U8(v) => {
                                        if _oper(&ck.operator, *v as i64, *i) {
                                            verified = true;
                                            if !checks_all {
                                                break 'params;
                                            }
                                        } else {
                                            verified = false;
                                            if checks_all {
                                                break 'params;
                                            }
                                        }
                                    }
                                    zvariant::Value::I16(v) => {
                                        if _oper(&ck.operator, *v as i64, *i) {
                                            verified = true;
                                            if !checks_all {
                                                break 'params;
                                            }
                                        } else {
                                            verified = false;
                                            if checks_all {
                                                break 'params;
                                            }
                                        }
                                    }
                                    zvariant::Value::U16(v) => {
                                        if _oper(&ck.operator, *v as i64, *i) {
                                            verified = true;
                                            if !checks_all {
                                                break 'params;
                                            }
                                        } else {
                                            verified = false;
                                            if checks_all {
                                                break 'params;
                                            }
                                        }
                                    }
                                    zvariant::Value::I32(v) => {
                                        if _oper(&ck.operator, *v as i64, *i) {
                                            verified = true;
                                            if !checks_all {
                                                break 'params;
                                            }
                                        } else {
                                            verified = false;
                                            if checks_all {
                                                break 'params;
                                            }
                                        }
                                    }
                                    zvariant::Value::U32(v) => {
                                        if _oper(&ck.operator, *v as i64, *i) {
                                            verified = true;
                                            if !checks_all {
                                                break 'params;
                                            }
                                        } else {
                                            verified = false;
                                            if checks_all {
                                                break 'params;
                                            }
                                        }
                                    }
                                    zvariant::Value::I64(v) => {
                                        if _oper(&ck.operator, *v, *i) {
                                            verified = true;
                                            if !checks_all {
                                                break 'params;
                                            }
                                        } else {
                                            verified = false;
                                            if checks_all {
                                                break 'params;
                                            }
                                        }
                                    }
                                    zvariant::Value::U64(v) => {
                                        if _oper(&ck.operator, *v as i128, *i as i128) {
                                            verified = true;
                                            if !checks_all {
                                                break 'params;
                                            }
                                        } else {
                                            verified = false;
                                            if checks_all {
                                                break 'params;
                                            }
                                        }
                                    }
                                    zvariant::Value::F64(v) => {
                                        if _oper(&ck.operator, *v, *i as f64) {
                                            verified = true;
                                            if !checks_all {
                                                break 'params;
                                            }
                                        } else {
                                            verified = false;
                                            if checks_all {
                                                break 'params;
                                            }
                                        }
                                    }
                                    zvariant::Value::Array(_) => {
                                        if ck.operator == ParamCheckOperator::Contains {
                                            if _contained_in(i, field_value) {
                                                verified = true;
                                                if !checks_all {
                                                    break 'params;
                                                }
                                            } else {
                                                verified = false;
                                                if checks_all {
                                                    break 'params;
                                                }
                                            }
                                        } else if ck.operator == ParamCheckOperator::NotContains {
                                            if !_contained_in(i, field_value) {
                                                verified = true;
                                                if !checks_all {
                                                    break 'params;
                                                }
                                            } else {
                                                verified = false;
                                                if checks_all {
                                                    break 'params;
                                                }
                                            }
                                        } else {
                                            verified = false;
                                            break 'params;
                                        }
                                    }
                                    e => {
                                        severity = LogType::Warn;
                                        ref_log_when = LOG_WHEN_PROC;
                                        ref_log_status = LOG_STATUS_FAIL;
                                        log_message = format!(
                                            "mismatched result type: {} expected (got `{e:?}`)",
                                            if ck.operator == ParamCheckOperator::Contains
                                                || ck.operator == ParamCheckOperator::NotContains
                                            {
                                                "container"
                                            } else {
                                                "integer"
                                            },
                                        );
                                        verified = false;
                                        break 'params;
                                    }
                                },
                                ParameterCheckValue::Float(f) => match field_value {
                                    zvariant::Value::U8(v) => {
                                        if _oper(&ck.operator, *v as f64, *f) {
                                            verified = true;
                                            if !checks_all {
                                                break 'params;
                                            }
                                        } else {
                                            verified = false;
                                            if checks_all {
                                                break 'params;
                                            }
                                        }
                                    }
                                    zvariant::Value::I16(v) => {
                                        if _oper(&ck.operator, *v as f64, *f) {
                                            verified = true;
                                            if !checks_all {
                                                break 'params;
                                            }
                                        } else {
                                            verified = false;
                                            if checks_all {
                                                break 'params;
                                            }
                                        }
                                    }
                                    zvariant::Value::U16(v) => {
                                        if _oper(&ck.operator, *v as f64, *f) {
                                            verified = true;
                                            if !checks_all {
                                                break 'params;
                                            }
                                        } else {
                                            verified = false;
                                            if checks_all {
                                                break 'params;
                                            }
                                        }
                                    }
                                    zvariant::Value::I32(v) => {
                                        if _oper(&ck.operator, *v as f64, *f) {
                                            verified = true;
                                            if !checks_all {
                                                break 'params;
                                            }
                                        } else {
                                            verified = false;
                                            if checks_all {
                                                break 'params;
                                            }
                                        }
                                    }
                                    zvariant::Value::U32(v) => {
                                        if _oper(&ck.operator, *v as f64, *f) {
                                            verified = true;
                                            if !checks_all {
                                                break 'params;
                                            }
                                        } else {
                                            verified = false;
                                            if checks_all {
                                                break 'params;
                                            }
                                        }
                                    }
                                    zvariant::Value::I64(v) => {
                                        if _oper(&ck.operator, *v as f64, *f) {
                                            verified = true;
                                            if !checks_all {
                                                break 'params;
                                            }
                                        } else {
                                            verified = false;
                                            if checks_all {
                                                break 'params;
                                            }
                                        }
                                    }
                                    zvariant::Value::F64(v) => {
                                        if _oper(&ck.operator, *v, *f) {
                                            verified = true;
                                            if !checks_all {
                                                break 'params;
                                            }
                                        } else {
                                            verified = false;
                                            if checks_all {
                                                break 'params;
                                            }
                                        }
                                    }
                                    zvariant::Value::Array(_) => {
                                        if ck.operator == ParamCheckOperator::Contains {
                                            if _contained_in(f, field_value) {
                                                verified = true;
                                                if !checks_all {
                                                    break 'params;
                                                }
                                            } else {
                                                verified = false;
                                                if checks_all {
                                                    break 'params;
                                                }
                                            }
                                        } else if ck.operator == ParamCheckOperator::NotContains {
                                            if !_contained_in(f, field_value) {
                                                verified = true;
                                                if !checks_all {
                                                    break 'params;
                                                }
                                            } else {
                                                verified = false;
                                                if checks_all {
                                                    break 'params;
                                                }
                                            }
                                        } else {
                                            verified = false;
                                            break 'params;
                                        }
                                    }
                                    e => {
                                        severity = LogType::Warn;
                                        ref_log_when = LOG_WHEN_PROC;
                                        ref_log_status = LOG_STATUS_FAIL;
                                        log_message = format!(
                                            "mismatched result type: {} expected (got `{e:?}`)",
                                            if ck.operator == ParamCheckOperator::Contains
                                                || ck.operator == ParamCheckOperator::NotContains
                                            {
                                                "container"
                                            } else {
                                                "float"
                                            },
                                        );
                                        verified = false;
                                        break 'params;
                                    }
                                },
                                ParameterCheckValue::String(s) => {
                                    if ck.operator == ParamCheckOperator::Equal {
                                        match field_value {
                                            zvariant::Value::Str(v) => {
                                                if *s == v.to_string() {
                                                    verified = true;
                                                    if !checks_all {
                                                        break 'params;
                                                    }
                                                } else {
                                                    verified = false;
                                                    if checks_all {
                                                        break 'params;
                                                    }
                                                }
                                            }
                                            zvariant::Value::ObjectPath(v) => {
                                                if *s == v.to_string() {
                                                    verified = true;
                                                    if !checks_all {
                                                        break 'params;
                                                    }
                                                } else {
                                                    verified = false;
                                                    if checks_all {
                                                        break 'params;
                                                    }
                                                }
                                            }
                                            e => {
                                                severity = LogType::Warn;
                                                ref_log_when = LOG_WHEN_PROC;
                                                ref_log_status = LOG_STATUS_FAIL;
                                                log_message = format!(
                                                    "mismatched result type: string expected (got `{e:?}`)",
                                                );
                                                verified = false;
                                                break 'params;
                                            }
                                        }
                                    } else if ck.operator == ParamCheckOperator::NotEqual {
                                        match field_value {
                                            zvariant::Value::Str(v) => {
                                                if *s != v.to_string() {
                                                    verified = true;
                                                    if !checks_all {
                                                        break 'params;
                                                    }
                                                } else {
                                                    verified = false;
                                                    if checks_all {
                                                        break 'params;
                                                    }
                                                }
                                            }
                                            zvariant::Value::ObjectPath(v) => {
                                                if *s != v.to_string() {
                                                    verified = true;
                                                    if !checks_all {
                                                        break 'params;
                                                    }
                                                } else {
                                                    verified = false;
                                                    if checks_all {
                                                        break 'params;
                                                    }
                                                }
                                            }
                                            e => {
                                                severity = LogType::Warn;
                                                ref_log_when = LOG_WHEN_PROC;
                                                ref_log_status = LOG_STATUS_FAIL;
                                                log_message = format!(
                                                    "mismatched result type: string expected (got `{e:?}`)",
                                                );
                                                verified = false;
                                                break 'params;
                                            }
                                        }
                                    } else if ck.operator == ParamCheckOperator::Contains {
                                        if _contained_in(s, field_value) {
                                            verified = true;
                                            if !checks_all {
                                                break 'params;
                                            }
                                        } else {
                                            verified = false;
                                            if checks_all {
                                                break 'params;
                                            }
                                        }
                                    } else if ck.operator == ParamCheckOperator::NotContains {
                                        if !_contained_in(s, field_value) {
                                            verified = true;
                                            if !checks_all {
                                                break 'params;
                                            }
                                        } else {
                                            verified = false;
                                            if checks_all {
                                                break 'params;
                                            }
                                        }
                                    } else {
                                        severity = LogType::Warn;
                                        ref_log_when = LOG_WHEN_PROC;
                                        ref_log_status = LOG_STATUS_FAIL;
                                        log_message = String::from("invalid operator for string");
                                        verified = false;
                                        break 'params;
                                    }
                                }
                                ParameterCheckValue::Regex(re) => {
                                    if ck.operator == ParamCheckOperator::Match {
                                        match field_value {
                                            zvariant::Value::Str(v) => {
                                                if re.is_match(v.as_str()) {
                                                    verified = true;
                                                    if !checks_all {
                                                        break 'params;
                                                    }
                                                } else {
                                                    verified = false;
                                                    if checks_all {
                                                        break 'params;
                                                    }
                                                }
                                            }
                                            zvariant::Value::ObjectPath(v) => {
                                                if re.is_match(v.as_str()) {
                                                    verified = true;
                                                    if !checks_all {
                                                        break 'params;
                                                    }
                                                } else {
                                                    verified = false;
                                                    if checks_all {
                                                        break 'params;
                                                    }
                                                }
                                            }
                                            e => {
                                                severity = LogType::Warn;
                                                ref_log_when = LOG_WHEN_PROC;
                                                ref_log_status = LOG_STATUS_FAIL;
                                                log_message = format!(
                                                    "mismatched result type: string expected (got `{e:?}`)",
                                                );
                                                verified = false;
                                                break 'params;
                                            }
                                        }
                                    } else {
                                        severity = LogType::Warn;
                                        ref_log_when = LOG_WHEN_PROC;
                                        ref_log_status = LOG_STATUS_FAIL;
                                        log_message =
                                            String::from("invalid operator for regular expression");
                                        verified = false;
                                        break 'params;
                                    }
                                }
                            }
                        }
                        _ => {
                            severity = LogType::Warn;
                            ref_log_when = LOG_WHEN_PROC;
                            ref_log_status = LOG_STATUS_FAIL;
                            log_message = String::from(
                                "could not retrieve field index: first index must be integer",
                            );
                            verified = false;
                            break;
                        }
                    }
                } else {
                    severity = LogType::Warn;
                    ref_log_when = LOG_WHEN_PROC;
                    ref_log_status = LOG_STATUS_FAIL;
                    log_message =
                        String::from("could not retrieve parameter: missing argument number");
                    verified = false;
                    break;
                }
            }
        } else {
            severity = LogType::Warn;
            ref_log_when = LOG_WHEN_PROC;
            ref_log_status = LOG_STATUS_FAIL;
            log_message = String::from("could not retrieve message body");
        }

        // the return value, including the aforementioned log message
        (
            verified,
            severity,
            ref_log_when,
            ref_log_status,
            log_message,
        )
    }
}

/// Infrastructure for performing checks on WMI results: since these checks
/// are slightly different from the ones that can be performed on DBus ones
/// (for instance, no returned arrays are taken into account, only simple
/// values), the operator and check types will be kept separated with no
/// common base
#[cfg(windows)]
#[cfg(feature = "wmi")]
#[allow(dead_code)]
pub mod wmiitem {
    use crate::LogType;
    use crate::constants::*;
    use regex::Regex;
    use std::collections::HashMap;
    use std::hash::{Hash, Hasher};

    use wmi::Variant;

    #[derive(PartialEq, Hash, Clone, Debug)]
    pub enum ResultCheckOperator {
        Equal,        // "eq"
        NotEqual,     // "neq"
        Greater,      // "gt"
        GreaterEqual, // "ge"
        Less,         // "lt"
        LessEqual,    // "le"
        Match,        // "match"
    }

    /// an enum containing the value that the result should be checked
    /// against
    #[derive(Debug)]
    pub enum ResultCheckValue {
        Boolean(bool),
        Integer(i64),
        Float(f64),
        String(String),
        Regex(Regex),
    }

    #[derive(Debug)]
    pub struct ResultCheckTest {
        pub index: Option<usize>, // `None` means any record
        pub field: String,
        pub operator: ResultCheckOperator,
        pub value: ResultCheckValue,
    }

    // implement the hash protocol for ResultCheckTest
    impl Hash for ResultCheckTest {
        fn hash<H: Hasher>(&self, state: &mut H) {
            self.index.hash(state);
            self.field.hash(state);
            self.operator.hash(state);
            match &self.value {
                ResultCheckValue::Boolean(x) => x.hash(state),
                ResultCheckValue::Integer(x) => x.hash(state),
                ResultCheckValue::Float(x) => x.to_bits().hash(state),
                ResultCheckValue::String(x) => x.hash(state),
                ResultCheckValue::Regex(x) => x.as_str().hash(state),
            }
        }
    }

    // allow a test to be easily cloned
    impl Clone for ResultCheckTest {
        fn clone(&self) -> Self {
            let value = match &self.value {
                ResultCheckValue::Boolean(x) => ResultCheckValue::Boolean(*x),
                ResultCheckValue::Integer(x) => ResultCheckValue::Integer(*x),
                ResultCheckValue::Float(x) => ResultCheckValue::Float(*x),
                ResultCheckValue::String(s) => ResultCheckValue::String(s.clone()),
                ResultCheckValue::Regex(e) => ResultCheckValue::Regex(e.clone()),
            };

            ResultCheckTest {
                index: self.index,
                field: self.field.clone(),
                operator: self.operator.clone(),
                value,
            }
        }
    }

    // the _oper helper to make code more readable is implemented here too
    fn _oper<T: PartialOrd + PartialEq>(op: &ResultCheckOperator, o1: T, o2: T) -> bool {
        match op {
            ResultCheckOperator::Equal => o1 == o2,
            ResultCheckOperator::NotEqual => o1 != o2,
            ResultCheckOperator::Less => o1 < o2,
            ResultCheckOperator::LessEqual => o1 <= o2,
            ResultCheckOperator::Greater => o1 > o2,
            ResultCheckOperator::GreaterEqual => o1 >= o2,
            ResultCheckOperator::Match => false,
        }
    }

    fn _check_variant(c: &ResultCheckValue, op: &ResultCheckOperator, v: &Variant) -> bool {
        match c {
            ResultCheckValue::Boolean(x) => match v {
                Variant::Bool(y) => _oper(op, x, y),
                _ => false,
            },
            ResultCheckValue::Integer(x) => match v {
                Variant::I1(y) => _oper(op, &(*y as i64), x),
                Variant::I2(y) => _oper(op, &(*y as i64), x),
                Variant::I4(y) => _oper(op, &(*y as i64), x),
                Variant::I8(y) => _oper(op, &(*y as i64), x),
                Variant::UI1(y) => _oper(op, &(*y as i64), x),
                Variant::UI2(y) => _oper(op, &(*y as i64), x),
                Variant::UI4(y) => _oper(op, &(*y as i64), x),
                Variant::UI8(y) => {
                    if *y > i64::MAX as u64 {
                        false
                    } else {
                        _oper(op, &(*y as i64), x)
                    }
                }
                Variant::R4(y) => _oper(op, y, &(*x as f32)),
                Variant::R8(y) => _oper(op, y, &(*x as f64)),
                _ => false,
            },
            ResultCheckValue::Float(x) => match v {
                Variant::I1(y) => _oper(op, &(*y as f64), x),
                Variant::I2(y) => _oper(op, &(*y as f64), x),
                Variant::I4(y) => _oper(op, &(*y as f64), x),
                Variant::I8(y) => _oper(op, &(*y as f64), x),
                Variant::UI1(y) => _oper(op, &(*y as f64), x),
                Variant::UI2(y) => _oper(op, &(*y as f64), x),
                Variant::UI4(y) => _oper(op, &(*y as f64), x),
                Variant::UI8(y) => _oper(op, &(*y as f64), x),
                Variant::R4(y) => _oper(op, &(*y as f64), x),
                Variant::R8(y) => _oper(op, &(*y as f64), x),
                _ => false,
            },
            ResultCheckValue::String(x) => match v {
                Variant::String(y) => _oper(op, y.as_str(), x.as_str()),
                _ => false,
            },
            ResultCheckValue::Regex(x) => {
                if *op == ResultCheckOperator::Match {
                    match v {
                        Variant::String(y) => x.is_match(y),
                        _ => false,
                    }
                } else {
                    false
                }
            }
        }
    }

    /// This is the heart of WMI result checks: it takes a reference to a raw
    /// WMI query result (see https://docs.rs/wmi/latest/wmi/ for reference)
    /// and a reference to the list of checks that has been configured, and
    /// performs the checks (all or some depends on the value of `checks_all`)
    /// on the array of records returned by the query.
    ///
    /// It returns a boolean that specifies if the check was successful or not
    /// (under the condition that either some or all the checks had to be
    /// tested), and four other values that form the variable part of a log
    /// message, suitable for being issued by the specialized logging function
    /// defined by the item.
    pub fn wmi_check_result(
        result: &Vec<HashMap<String, Variant>>, // the dbus message
        checks: &[ResultCheckTest],             // item.result_checks
        checks_all: bool,                       // item.checks_all
    ) -> (
        bool,         // verified
        LogType,      // the log severity
        &'static str, // log/when (LOG_WHEN_...)
        &'static str, // log/status (LOG_STATUS_...)
        String,       // log message
    ) {
        let mut verified: bool = checks_all;
        let mut severity: LogType = LogType::Trace;
        let mut ref_log_when: &str = LOG_WHEN_PROC;
        let mut ref_log_status: &str = LOG_STATUS_OK;
        let mut log_message: String = String::from("result check ended");

        'result: for ck in checks.iter() {
            if let Some(idx) = ck.index {
                if idx > result.len() {
                    severity = LogType::Warn;
                    ref_log_when = LOG_WHEN_PROC;
                    ref_log_status = LOG_STATUS_FAIL;
                    log_message = format!("could not retrieve result: index {idx} out of range");
                    verified = false;
                    break 'result;
                } else {
                    let rec = result.get(idx).unwrap();
                    if let Some(v) = rec.get(&ck.field) {
                        if !_check_variant(&ck.value, &ck.operator, v) {
                            verified = false;
                            break 'result;
                        } else if !checks_all {
                            verified = true;
                            break 'result;
                        }
                    } else {
                        severity = LogType::Warn;
                        ref_log_when = LOG_WHEN_PROC;
                        ref_log_status = LOG_STATUS_FAIL;
                        log_message = format!(
                            "could not check result: field `{}` not found in record",
                            ck.field,
                        );
                        verified = false;
                        break 'result;
                    }
                }
            } else {
                for rec in result {
                    if let Some(v) = rec.get(&ck.field) {
                        if !_check_variant(&ck.value, &ck.operator, v) {
                            verified = false;
                            break 'result;
                        } else if !checks_all {
                            verified = true;
                            break 'result;
                        }
                    } else {
                        severity = LogType::Warn;
                        ref_log_when = LOG_WHEN_PROC;
                        ref_log_status = LOG_STATUS_FAIL;
                        log_message = format!(
                            "could not check result: field `{}` not found in record",
                            ck.field,
                        );
                        verified = false;
                        break 'result;
                    }
                }
            }
        }

        // the return value, including the aforementioned log message
        (
            verified,
            severity,
            ref_log_when,
            ref_log_status,
            log_message,
        )
    }
}

/// A common result type: catching errors from modules used throughout
/// the entire code. The corresponding error carries some information
/// about what went wrong.
#[allow(dead_code)]
pub mod wres {
    use notify;
    use std::{self, fmt};

    use crate::constants::ERR_FAILED;

    /// Types of specific errors
    #[non_exhaustive]
    #[derive(Debug, Clone)]
    pub enum Kind {
        Forbidden,
        Unsupported,
        Unavailable,
        Unconverted,
        Unparsed,
        Busy,
        Invalid,
        Failed,
        Empty,
        // ...
        Unknown,
    }

    impl fmt::Display for Kind {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(
                f,
                "{}",
                match self {
                    Kind::Forbidden => "not permitted",
                    Kind::Unsupported => "not supported",
                    Kind::Unavailable => "not available",
                    Kind::Unconverted => "not converted",
                    Kind::Unparsed => "not parsed",
                    Kind::Busy => "resource busy",
                    Kind::Invalid => "invalid",
                    Kind::Failed => "failed",
                    Kind::Empty => "empty",
                    Kind::Unknown => "unknown",
                }
            )
        }
    }

    /// Describes the origin of the error: if `Native` the error was originated
    /// natively, otherwise the field is set by another error that is converted
    /// into `Error` via a dedicated `From` trait implementation.
    #[non_exhaustive]
    #[derive(Debug, Clone, PartialEq)]
    pub enum Origin {
        Native,
        Unit,
        StdIo,
        Notify,

        #[cfg(feature = "dbus")]
        DBus,

        #[cfg(windows)]
        #[cfg(feature = "wmi")]
        Wmi,

        // ...
        Unknown,
    }

    impl fmt::Display for Origin {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(
                f,
                "{}",
                match self {
                    Origin::Native => "self",
                    Origin::Unit => "unit",
                    Origin::StdIo => "io",
                    Origin::Notify => "fschange",

                    #[cfg(feature = "dbus")]
                    Origin::DBus => "dbus",

                    #[cfg(windows)]
                    #[cfg(feature = "wmi")]
                    Origin::Wmi => "wmi",

                    // ...
                    Origin::Unknown => "unknown",
                }
            )
        }
    }

    /// The error type that is used throughout the application: implementations
    /// of the `From` trait are used to implicitly convert from other error
    /// types, which in turn set the `origin` property.
    #[derive(Debug, Clone)]
    pub struct Error {
        kind: Kind,
        origin: Origin,
        message: String, // freeform message: owned in order to avoid lifetime management
    }

    impl fmt::Display for Error {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            if self.origin != Origin::Native {
                write!(f, "{} ({}): {}", &self.kind, &self.origin, &self.message)
            } else {
                write!(f, "{}: {}", &self.kind, &self.message)
            }
        }
    }

    // maybe the most important: From<std::io::Error>
    impl From<std::io::Error> for Error {
        fn from(e: std::io::Error) -> Self {
            Self {
                kind: match e.kind() {
                    std::io::ErrorKind::Unsupported => Kind::Unsupported,
                    std::io::ErrorKind::PermissionDenied => Kind::Forbidden,
                    std::io::ErrorKind::InvalidData => Kind::Invalid,
                    std::io::ErrorKind::InvalidInput => Kind::Invalid,
                    _ => Kind::Unknown,
                },
                origin: Origin::StdIo,
                message: e.to_string(),
            }
        }
    }

    // notify (fschange) errors
    impl From<notify::Error> for Error {
        fn from(e: notify::Error) -> Self {
            Self {
                kind: Kind::Failed,
                origin: Origin::Notify,
                message: e.to_string(),
            }
        }
    }

    // zbus errors
    #[cfg(feature = "dbus")]
    impl From<zbus::Error> for Error {
        fn from(e: zbus::Error) -> Self {
            Self {
                kind: Kind::Failed,
                origin: Origin::DBus,
                message: e.to_string(),
            }
        }
    }

    // wmi errors
    #[cfg(windows)]
    #[cfg(feature = "wmi")]
    impl From<wmi::WMIError> for Error {
        fn from(e: wmi::WMIError) -> Self {
            let kind = match e {
                wmi::WMIError::ConvertBoolError(_)
                | wmi::WMIError::ConvertStringError(_)
                | wmi::WMIError::ConvertLengthError(_)
                | wmi::WMIError::ConvertDatetimeError(_)
                | wmi::WMIError::ConvertDurationError(_)
                | wmi::WMIError::ConvertVariantError(_)
                | wmi::WMIError::ConvertError(_) => Kind::Unconverted,
                wmi::WMIError::DeserializeValueError(_)
                | wmi::WMIError::InvalidDeserializationVariantError(_)
                | wmi::WMIError::SerdeError(_) => Kind::Invalid,
                wmi::WMIError::ParseDatetimeError(_)
                | wmi::WMIError::ParseFloatError(_)
                | wmi::WMIError::ParseIntError(_) => Kind::Unparsed,
                wmi::WMIError::UnimplementedArrayItem => Kind::Unavailable,
                _ => Kind::Failed,
            };
            Self {
                kind,
                origin: Origin::Wmi,
                message: e.to_string(),
            }
        }
    }

    // errors based on the unit type
    impl From<()> for Error {
        fn from(_: ()) -> Self {
            Self {
                kind: Kind::Failed,
                origin: Origin::Unit,
                message: ERR_FAILED.to_owned(),
            }
        }
    }

    // implements `Error` and provides access to properties
    impl Error {
        // this is used only to natively create an instance of `Error`: only
        // conversions set the `origin` property to something different
        pub fn new(kind: Kind, message: &str) -> Self {
            Self {
                kind,
                origin: Origin::Native,
                message: message.to_string(),
            }
        }

        // property access
        pub fn kind(&self) -> &Kind {
            &self.kind
        }

        pub fn origin(&self) -> &Origin {
            &self.origin
        }

        pub fn message(&self) -> &str {
            &self.message
        }
    }

    /// Specific `Result` type that assumes `wres::Error` as its Err variant
    pub type Result<T> = std::result::Result<T, Error>;

    // FIXME: enabling the following creates a conflict with other derived
    // error types that are implemented as specific cases
    //
    // this should be the last resort impl
    // impl<T: error::Error + Send + Sync + 'static> From<T> for Error {
    //     fn from(e: T) -> Self {
    //         Self {
    //             kind: Kind::Unknown,
    //             was: Was::Unknown,
    //             message: e.to_string(),
    //         }
    //     }
    // }
}

// end.
