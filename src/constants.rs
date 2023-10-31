//! pub constants
//!
//! Various public string constants used throughout the crate, mostly error
//! messages or other kinds of hints. Also, expose some regular expressions
//! that are used to identify various items, to all modules.



use lazy_static::lazy_static;
use regex::Regex;
use std::time::Duration;


/// The application name
pub const APP_NAME: &str = "whenever";


#[allow(dead_code)]
// error messages
pub const ERR_OK: &str = "OK";
pub const ERR_INVALID_CONFIG_FILE: &str = "invalid configuration file";
pub const ERR_INVALID_TASK_CONFIG: &str = "invalid task configuration";
pub const ERR_INVALID_COND_CONFIG: &str = "invalid condition configuration";
pub const ERR_INVALID_EVENT_CONFIG: &str = "invalid event configuration";
pub const ERR_TASKREG_TASK_NOT_ADDED: &str = "could not add task to the registry";
pub const ERR_TASKREG_CANNOT_PULL_TASK: &str = "could not pull task out from the registry";
pub const ERR_CONDREG_COND_NOT_ADDED: &str = "could not add condition to the registry";
pub const ERR_CONDREG_CANNOT_PULL_COND: &str = "could not pull condition out from the registry";
pub const ERR_CONDREG_COND_RESET_BUSY: &str = "attempt to reset condition while busy";
pub const ERR_CONDREG_COND_SUSPEND_BUSY: &str = "attempt to suspend condition while busy";
pub const ERR_CONDREG_COND_RESUME_BUSY: &str = "attempt to resume condition while busy";
pub const ERR_EVENTREG_EVENT_NOT_ADDED: &str = "could not add event to the registry";
pub const ERR_EVENTREG_CANNOT_PULL_EVENT: &str = "could not pull event out from the registry";

pub const ERR_COND_CANNOT_RESET: &str = "condition could not reset status";
pub const ERR_COND_CANNOT_SET_SUCCESS: &str = "condition could not set success status";
pub const ERR_COND_CANNOT_SET_CHECKED: &str = "condition could not set checked status";
pub const ERR_COND_TASK_NOT_ADDED: &str = "condition could not add task";
pub const ERR_COND_CANNOT_CONNECT_TO: &str = "could not estabilish connection to";

pub const ERR_EVENT_INVALID_MATCH_RULE: &str = "invalid event match rule";
pub const ERR_EVENT_INVALID_COND_TYPE: &str = "invalid condition type for assignment to event";

pub const ERR_TIMEOUT_REACHED: &str = "timeout reached";
pub const ERR_UNKNOWN_EXITSTATUS: &str = "unknown exit status";
pub const ERR_ALREADY_RUNNING: &str = "another instance of the scheduler is already running";
pub const ERR_LOGGER_NOT_INITIALIZED: &str = "could not initialize logger";

pub const ERR_INVALID_CFG_ENTRY: &str = "invalid configuration entry";
pub const ERR_MISSING_PARAMETER: &str = "missing parameter";
pub const ERR_INVALID_PARAMETER: &str = "invalid parameter";
pub const ERR_INVALID_COND_NAME: &str = "invalid value for condition name";
pub const ERR_INVALID_COND_TYPE: &str = "condition type invalid or mismatched";
pub const ERR_INVALID_TASK_LIST: &str = "invalid task list specification";
pub const ERR_INVALID_TASK: &str = "invalid task specification or inexistent task";

pub const ERR_INVALID_STARTUP_PATH: &str = "invalid startup path";
pub const ERR_INVALID_COMMAND_PATH: &str = "invalid command path";
pub const ERR_INVALID_COMMAND_ARGUMENTS: &str = "invalid command arguments";
pub const ERR_INVALID_ENVVAR_NAME: &str = "invalid name for environment variable";
pub const ERR_INVALID_ENVVAR_VALUE: &str = "invalid value for environment variable";

pub const ERR_INVALID_VAR_NAME: &str = "invalid variable name";
pub const ERR_INVALID_VAR_VALUE: &str = "invalid variable value";

pub const ERR_INVALID_TIMESPEC: &str = "invalid specification for date or time";
pub const ERR_INVALID_TICK_SECONDS: &str = "invalid number of seconds for tick";
pub const ERR_INVALID_VALUE_FOR: &str = "invalid value for";
pub const ERR_INVALID_VALUE_FOR_ENTRY: &str = "invalid value for entry";

pub const ERR_INVALID_TASK_NAME: &str = "invalid value for task name";
pub const ERR_INVALID_TASK_TYPE: &str = "task type invalid or mismatched";

pub const ERR_INVALID_EVENT_NAME: &str = "invalid value for event name";
pub const ERR_INVALID_EVENT_TYPE: &str = "event type invalid or mismatched";
pub const ERR_INVALID_EVENT_CONDITION: &str = "condition not found for event";


// logging constants
pub const LOG_WHEN_INIT: &str = "INIT";
pub const LOG_WHEN_START: &str = "START";
pub const LOG_WHEN_END: &str = "END";
pub const LOG_WHEN_PROC: &str = "PROC";
pub const LOG_WHEN_HISTORY: &str = "HIST";

pub const LOG_STATUS_OK: &str = "OK";
pub const LOG_STATUS_FAIL: &str = "FAIL";
pub const LOG_STATUS_MSG: &str = "MSG";
pub const LOG_STATUS_ERR: &str = "ERR";
pub const LOG_STATUS_HIST_START: &str = "START";
pub const LOG_STATUS_HIST_END: &str = "END";


// other string pub constants
pub const STR_UNKNOWN_VALUE: &str = "<unknown>";


// crate-wide values
lazy_static! {
    // environment variables set by the command based task
    pub static ref ENVVAR_NAME_TASK: String = format!("{}_TASK", APP_NAME.to_ascii_uppercase());
    pub static ref ENVVAR_NAME_COND: String = format!("{}_CONDITION", APP_NAME.to_ascii_uppercase());

    pub static ref RE_TASK_NAME: Regex = Regex::new(r"^[a-zA-Z_][a-zA-Z0-9_]*$").unwrap();
    pub static ref RE_COND_NAME: Regex = Regex::new(r"^[a-zA-Z_][a-zA-Z0-9_]*$").unwrap();
    pub static ref RE_EVENT_NAME: Regex = Regex::new(r"^[a-zA-Z_][a-zA-Z0-9_]*$").unwrap();
    pub static ref RE_VAR_NAME: Regex = Regex::new(r"^[a-zA-Z_][a-zA-Z0-9_]*$").unwrap();
    pub static ref RE_ENVVAR_NAME: Regex = Regex::new(r"^[a-zA-Z_][a-zA-Z0-9_]*$").unwrap();

    pub static ref LUAVAR_NAME_TASK: String = format!("{}_task", APP_NAME.to_ascii_lowercase());
    pub static ref LUAVAR_NAME_COND: String = format!("{}_condition", APP_NAME.to_ascii_lowercase());

    // DBus names regular expressions (see https://dbus.freedesktop.org/doc/dbus-specification.html)
    // Note that bus names adhere to specification as in (quoting): "only
    // elements that are part of a unique connection name may begin with a
    // digit", so the case must be taken into account. The other definitions
    // follow the above mentioned specification: two separate REs are given
    // for interfaces and errors even though, per specification, both follow
    // the same naming rules. Same yields for service names.
    // Note: although the bus name can be checked through a RE, the only
    //       supported names are actually ":system" and ":session"
    pub static ref RE_DBUS_BUS_NAME: Regex = Regex::new(r"^\:[a-zA-Z0-9_-]+(\.[a-zA-Z0-9_-]+)+$").unwrap();
    pub static ref RE_DBUS_MSGBUS_NAME: Regex = Regex::new(r"^\:(session|system)$").unwrap();
    pub static ref RE_DBUS_SERVICE_NAME: Regex = Regex::new(r"^[a-zA-Z_][a-zA-Z0-9_]*(\.[a-zA-Z_][a-zA-Z0-9_]*)+$").unwrap();
    pub static ref RE_DBUS_OBJECT_PATH: Regex = Regex::new(r"^/([a-zA-Z0-9_]+(/[a-zA-Z0-9_]+)*)?$").unwrap();
    pub static ref RE_DBUS_INTERFACE_NAME: Regex = Regex::new(r"^[a-zA-Z_][a-zA-Z0-9_]*(\.[a-zA-Z_][a-zA-Z0-9_]*)+$").unwrap();
    pub static ref RE_DBUS_MEMBER_NAME: Regex = Regex::new(r"^[a-zA-Z_][a-zA-Z0-9_]*$").unwrap();
    pub static ref RE_DBUS_ERROR_NAME: Regex = Regex::new(r"^[a-zA-Z_][a-zA-Z0-9_]*(\.[a-zA-Z_][a-zA-Z0-9_]*)+$").unwrap();

    // interval for polling spawned commands for stdout/stderr contents
    pub static ref DUR_SPAWNED_POLL_INTERVAL: Duration = Duration::from_millis(100);

}


// end.
