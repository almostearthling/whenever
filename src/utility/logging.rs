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
use parking_lot::RwLock;

use crate::constants::{APP_NAME, ERR_LOGGER_NOT_INITIALIZED};
use flexi_logger::{DeferredNow, FileSpec, Logger, style};
use log::Record;
use log::{debug, error, info, trace, warn};
use nu_ansi_term::Style;
use serde_json::json;
use std::path::PathBuf;

// the following global flag is exposed here because it looks like there is
// no actual way to pass anything but a string as payload to the logger, so
// the common logging function should know whether the logger is initialized
// to return JSON message and build the JSON payload itself
lazy_static! {
    static ref LOGGER_EMITS_JSON: RwLock<bool> = RwLock::new(false);
}

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
        record.args(),
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
                        *LOGGER_EMITS_JSON.write() = true;
                        log_format_json
                    } else {
                        log_format_plain
                    }
                };
                let mut pb = PathBuf::from(&fname);
                if pb.parent().is_none() || pb.parent().unwrap().to_str().unwrap_or("").is_empty() {
                    pb = {
                        let mut dir = PathBuf::from(".");
                        dir.push(pb);
                        dir
                    }
                }
                let fspec =
                    FileSpec::try_from(&pb).map_err(|e| std::io::Error::other(e.to_string()))?;
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
                        *LOGGER_EMITS_JSON.write() = true;
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
    let payload = if *LOGGER_EMITS_JSON.read() {
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
