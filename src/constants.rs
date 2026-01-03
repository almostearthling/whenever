//! pub constants
//!
//! Various public string constants used throughout the crate, mostly error
//! messages or other kinds of hints. Also, expose some regular expressions
//! that are used to identify various items, to all modules.
#![allow(dead_code)]

use lazy_static::lazy_static;
use regex::Regex;
use std::time::Duration;

// The application name
pub const APP_NAME: &str = env!("CARGO_PKG_NAME");
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

// The application GUID in order to force a single instance: different GUIDs
// are used in order to be able to launch a DEBUG instance when a release
// instance is running: for developers, to be able to run quick tests without
// having to stop a running instance
#[cfg(debug_assertions)]
pub const APP_GUID: &str = "663f98a9-a1ef-46ef-a7bc-bb2482f42440_DEBUG";

#[cfg(not(debug_assertions))]
pub const APP_GUID: &str = "663f98a9-a1ef-46ef-a7bc-bb2482f42440";

#[allow(dead_code)]
// error messages
pub const ERR_OK: &str = "OK";
pub const ERR_FAILED: &str = "failed";
pub const ERR_INVALID_CONFIG_FILE: &str = "invalid configuration file";
pub const ERR_INVALID_TASK_CONFIG: &str = "invalid task configuration";
pub const ERR_INVALID_COND_CONFIG: &str = "invalid condition configuration";
pub const ERR_INVALID_EVENT_CONFIG: &str = "invalid event configuration";
pub const ERR_INVALID_CONFIG: &str = "invalid configuration";
pub const ERR_TASKREG_TASK_NOT_ADDED: &str = "could not add task to the registry";
pub const ERR_TASKREG_TASK_NOT_REPLACED: &str = "could not replace task in the registry";
pub const ERR_TASKREG_CANNOT_PULL_TASK: &str = "could not pull task out from the registry";
pub const ERR_CONDREG_COND_NOT_ADDED: &str = "could not add condition to the registry";
pub const ERR_CONDREG_COND_NOT_REPLACED: &str = "could not replace condition in the registry";
pub const ERR_CONDREG_CANNOT_PULL_COND: &str = "could not pull condition out from the registry";
pub const ERR_CONDREG_COND_RESET_BUSY: &str = "attempt to reset condition while busy";
pub const ERR_CONDREG_COND_SUSPEND_BUSY: &str = "attempt to suspend condition while busy";
pub const ERR_CONDREG_COND_RESUME_BUSY: &str = "attempt to resume condition while busy";
pub const ERR_EVENTREG_EVENT_NOT_ADDED: &str = "could not add event to the registry";
pub const ERR_EVENTREG_CANNOT_PULL_EVENT: &str = "could not pull event out from the registry";
pub const ERR_EVENTREG_CANNOT_REMOVE_EVENT: &str = "could not remove event from the registry";
pub const ERR_EVENTREG_CANNOT_STOP_LISTENER: &str = "could not shut down the event listener";

pub const ERR_COND_CANNOT_RESET: &str = "condition could not reset status";
pub const ERR_COND_CANNOT_SET_SUCCESS: &str = "condition could not set success status";
pub const ERR_COND_CANNOT_SET_CHECKED: &str = "condition could not set checked status";
pub const ERR_COND_TASK_NOT_ADDED: &str = "condition could not add task";

pub const ERR_EVENT_INVALID_COND_TYPE: &str = "invalid condition type for assignment to event";

pub const ERR_TIMEOUT_REACHED: &str = "timeout reached";
pub const ERR_UNKNOWN_EXITSTATUS: &str = "unknown exit status";
pub const ERR_ALREADY_RUNNING: &str = "another instance of the scheduler is already running";
pub const ERR_LOGGER_NOT_INITIALIZED: &str = "could not initialize logger";

pub const ERR_INVALID_CFG_ENTRY: &str = "invalid configuration entry";
pub const ERR_MISSING_PARAMETER: &str = "missing parameter";
pub const ERR_INVALID_PARAMETER: &str = "invalid parameter";
pub const ERR_INVALID_PARAMETER_LIST: &str = "invalid list or list element";
pub const ERR_INVALID_FILESPEC: &str = "invalid file specification";
pub const ERR_INVALID_COND_TYPE: &str = "condition type invalid or mismatched";
pub const ERR_INVALID_TASK: &str = "invalid task specification or inexistent task";

pub const ERR_INVALID_STARTUP_PATH: &str = "invalid startup path";
pub const ERR_INVALID_ENVVAR_NAME: &str = "invalid name for environment variable";
pub const ERR_INVALID_ENVVAR_VALUE: &str = "invalid value for environment variable";

pub const ERR_INVALID_VAR_NAME: &str = "invalid variable name";
pub const ERR_INVALID_VAR_VALUE: &str = "invalid variable value";

pub const ERR_INVALID_TIMESPEC: &str = "invalid specification for date or time";
pub const ERR_INVALID_TICK_SECONDS: &str = "invalid number of seconds for tick";
pub const ERR_INVALID_VALUE_FOR: &str = "invalid value for";
pub const ERR_INVALID_VALUE_FOR_ENTRY: &str = "invalid value for entry";
pub const ERR_INVALID_VALUE_FOR_LIST_ENTRY: &str = "invalid value for list entry";
pub const ERR_INVALID_CONFIG_FOR_ENTRY: &str = "cannot configure entry";

pub const ERR_INVALID_TASK_TYPE: &str = "task type invalid or mismatched";

pub const ERR_INVALID_EVENT_TYPE: &str = "event type invalid or mismatched";
pub const ERR_INVALID_EVENT_CONDITION: &str = "condition not found for event";

pub const ERR_EVENT_CAUGHT_INVALID: &str = "invalid event caught";
pub const ERR_EVENT_LISTENING_NOT_DETERMINED: &str =
    "could not determine whether the service is running";

// logging constants
pub const LOG_WHEN_INIT: &str = "INIT";
pub const LOG_WHEN_START: &str = "START";
pub const LOG_WHEN_END: &str = "END";
pub const LOG_WHEN_PROC: &str = "PROC";
pub const LOG_WHEN_HISTORY: &str = "HIST";
pub const LOG_WHEN_BUSY: &str = "BUSY";
pub const LOG_WHEN_PAUSE: &str = "PAUSE";

pub const LOG_STATUS_OK: &str = "OK";
pub const LOG_STATUS_FAIL: &str = "FAIL";
pub const LOG_STATUS_MSG: &str = "MSG";
pub const LOG_STATUS_ERR: &str = "ERR";
pub const LOG_STATUS_HIST_START: &str = "START";
pub const LOG_STATUS_HIST_END: &str = "END";
pub const LOG_STATUS_YES: &str = "YES";
pub const LOG_STATUS_NO: &str = "NO";

pub const LOG_EMITTER_TASK: &str = "TASK";
pub const LOG_EMITTER_TASK_REGISTRY: &str = "TASK_REGISTRY";
pub const LOG_EMITTER_CONDITION: &str = "CONDITION";
pub const LOG_EMITTER_CONDITION_REGISTRY: &str = "CONDITION_REGISTRY";
pub const LOG_EMITTER_EVENT: &str = "EVENT";
pub const LOG_EMITTER_EVENT_REGISTRY: &str = "EVENT_REGISTRY";
pub const LOG_EMITTER_CONFIGURATION: &str = "CONFIGURATION";
pub const LOG_EMITTER_MAIN: &str = "MAIN";

pub const LOG_EMITTER_TASK_COMMAND: &str = "COMMAND_TASK";
pub const LOG_EMITTER_TASK_LUA: &str = "LUA_TASK";
pub const LOG_EMITTER_TASK_INTERNAL: &str = "INTERNAL_TASK";

pub const LOG_EMITTER_EVENT_FSCHANGE: &str = "FSCHANGE_EVENT";
pub const LOG_EMITTER_EVENT_MANUAL: &str = "CMD_EVENT";
#[cfg(feature = "dbus")]
pub const LOG_EMITTER_EVENT_DBUS: &str = "DBUS_EVENT";
#[cfg(windows)]
#[cfg(feature = "wmi")]
pub const LOG_EMITTER_EVENT_WMI: &str = "WMI_EVENT";

pub const LOG_EMITTER_CONDITION_INTERVAL: &str = "INTERVAL_CONDITION";
pub const LOG_EMITTER_CONDITION_BUCKET: &str = "BUCKET_CONDITION";
pub const LOG_EMITTER_CONDITION_COMMAND: &str = "COMMAND_CONDITION";
pub const LOG_EMITTER_CONDITION_IDLE: &str = "IDLE_CONDITION";
pub const LOG_EMITTER_CONDITION_LUA: &str = "LUA_CONDITION";
#[cfg(feature = "dbus")]
pub const LOG_EMITTER_CONDITION_DBUS: &str = "DBUS_CONDITION";
#[cfg(windows)]
#[cfg(feature = "wmi")]
pub const LOG_EMITTER_CONDITION_WMI: &str = "WMI_CONDITION";

pub const LOG_ACTION_NEW: &str = "new";
pub const LOG_ACTION_TICK: &str = "tick";
pub const LOG_ACTION_FIRE: &str = "fire";
pub const LOG_ACTION_TRIGGER: &str = "trigger";
pub const LOG_ACTION_INSTALL: &str = "install";
pub const LOG_ACTION_UNINSTALL: &str = "uninstall";
pub const LOG_ACTION_RECONFIGURE: &str = "reconfigure";
pub const LOG_ACTION_ACTIVE: &str = "active";
pub const LOG_ACTION_LUA: &str = "exec_lua";
pub const LOG_ACTION_SCHEDULER_TICK: &str = "scheduler_tick";
pub const LOG_ACTION_RESET_CONDITIONS: &str = "reset_conditions";
pub const LOG_ACTION_SUSPEND_CONDITION: &str = "suspend_condition";
pub const LOG_ACTION_RESUME_CONDITION: &str = "resume_condition";
pub const LOG_ACTION_CONDITION_BUSY: &str = "condition_busy";
pub const LOG_ACTION_CONDITION_STATE: &str = "condition_state";
pub const LOG_ACTION_EVENT_TRIGGER: &str = "event_trigger";
pub const LOG_ACTION_RUN_COMMAND: &str = "command";
pub const LOG_ACTION_MAIN_LISTENER: &str = "listener";
pub const LOG_ACTION_MAIN_START: &str = "starting";
pub const LOG_ACTION_MAIN_EXIT: &str = "exit";
pub const LOG_ACTION_RUN_TASKS_SEQ: &str = "run_seq";
pub const LOG_ACTION_RUN_TASKS_PAR: &str = "run_par";

// other string pub constants
pub const STR_UNKNOWN_VALUE: &str = "<unknown>";
pub const STR_INVALID_TYPE: &str = "<invalid_type>";
pub const STR_INVALID_VALUE: &str = "<invalid_value>";

// default values
pub const DEFAULT_SCHEDULER_TICK_SECONDS: i64 = 5;
pub const DEFAULT_RANDOMIZE_CHECKS_WITHIN_TICKS: bool = false;

// operational values
pub const MAIN_STDIN_READ_WAIT_MILLISECONDS: u64 = 100; // default: 100
pub const MAIN_EVENT_REGISTRY_MGMT_MILLISECONDS: u64 = 100; // default: 100

// channel sizes
pub const EVENT_QUIT_CHANNEL_SIZE: usize = 10; // default: 10
pub const EVENT_CHANNEL_SIZE: usize = 10; // default: 10

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

    // interval for polling spawned commands for stdout/stderr contents
    pub static ref DUR_SPAWNED_POLL_INTERVAL: Duration = Duration::from_millis(100);

}

#[cfg(feature = "dbus")]
lazy_static! {
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
}

#[cfg(feature = "wmi")]
lazy_static! {
    // The regular expression for WMI namespaces is inferred from lists that
    // can be obtained from the command line using one of the methods found
    // here: https://stackoverflow.com/questions/5332501/how-do-you-query-for-wmi-namespaces
    // the check for the first part to be `ROOT\` is left to a frontend, we
    // only check that a string that can be used as a namespace is provided
    pub static ref RE_WMI_NAMESPACE: Regex = Regex::new(r"^[a-zA-Z_][a-zA-Z0-9_]*([/\\][a-zA-Z_][a-zA-Z0-9_]*)+$").unwrap();
}

// end.
