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
//!     `context: [MSGTYPE] human readable message`
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
//!          in this case _MSG_ is sent at the beginning of task execution, and
//!          _OK_, _FAIL_ or _IND_ are sent at the end (resp. on success, 
//!          failure or _indeterminate_ outcome)
//!
//! while the second can be one of:
//!
//! * _OK_ for expected outcomes or behaviours
//! * _FAIL_ for unexpected outcomes or behaviours
//! * _IND_ for indeterminate outcomes
//! * _MSG_ if the human-readable part is exclusively informational
//! * _ERR_ (may be followed by a dash `-` and a code) for errors to be
//!   notified.
//!
//! This should help using the log as a way of communicating to a wrapper
//! utility the state of the scheduler, and possibily give the opportunity to
//! organize communication to the user in a friendlier way.


/// The application name
pub const APP_NAME: &str = "whenever";


// an enum to produce different log messages used by the trait-defined logger

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
    use std::path::PathBuf;
    use log::{debug, error, info, warn, trace};
    use flexi_logger::{Logger, FileSpec, DeferredNow, style};
    use nu_ansi_term::Style;
    use log::Record;
    use serde_json::json;
    use crate::constants::{APP_NAME, ERR_LOGGER_NOT_INITIALIZED};

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
            format!("{:5}", record.level()),
            &record.args(),
        )
    }

    fn log_format_json(
        w: &mut dyn std::io::Write,
        now: &mut DeferredNow,
        record: &Record,
    ) -> Result<(), std::io::Error> {
        write!(
            w,
            "{}",
            json!({
                "application": APP_NAME,
                "time": now.format(NOW_FMT_FULL).to_string(),
                "level": record.level().to_string(),
                "message": &record.args().to_string(),
            })
        )
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
            format!("{}", now.format(NOW_FMT)),
            dimmed.paint(format!("({})", APP_NAME)),
            style(level).paint(format!("{:5}", level.to_string())),
            bold.paint(format!("{}", &record.args())),
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
        logcolor: bool,     // these three values are mutually
        logplain: bool,     // exclusive by construction of the
        logjson: bool,      // main `clap` parser
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
                            log_format_json
                        } else {
                            log_format_plain
                        }
                    };
                    let mut pb = PathBuf::from(&fname);
                    if pb.parent().is_none()
                    || pb.parent().unwrap().to_str().unwrap_or("").is_empty() {
                        pb = {
                            let mut dir = PathBuf::from(".");
                            dir.push(pb);
                            dir
                        }
                    }
                    let fspec = FileSpec::try_from(&pb).unwrap();
                    logger = Ok(l
                        .log_to_file(fspec)
                        .format_for_files(log_format)
                        // .write_mode(WriteMode::BufferAndFlush)
                    );
                    if append {
                        logger = Ok(logger
                            .unwrap()
                            .append()
                        );
                    }
                } else {
                    let log_format = {
                        if logcolor {
                            log_format_colors
                        } else if logplain {
                            log_format_plain
                        } else if logjson {
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
                            .log_to_stdout()
                        );
                    } else {
                        logger = Ok(l
                            .format_for_stdout(log_format)
                            .log_to_stdout()
                        );
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


    /// Common log function: `context` specifies which part of the application
    /// originated the message, and `message` is the actual information
    pub fn log(severity: LogType, context: &str, message: &str) {
        match severity {
            LogType::Trace => { trace!("{context}: {message}") }
            LogType::Debug => { debug!("{context}: {message}") }
            LogType::Info => { info!("{context}: {message}") }
            LogType::Warn => { warn!("{context}: {message}") }
            LogType::Error => { error!("{context}: {message}") }
        }
    }

}


// end.
