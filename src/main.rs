//! # whenever
//!
//! A simple multiplatform background job launcher based upon verification of
//! various types of conditions.
//!
//! It is intended as a 100% Rust successor to the core part of the Python
//! based [When](https://github.com/almostearthling/when-command) utility.


use std::io::{stdin, Stdin, BufRead};
use std::thread;
use std::sync::RwLock;
use std::time::Duration;
use std::thread::JoinHandle;

use lazy_static::lazy_static;

use clokwerk::{Scheduler, TimeUnits};
use cfgmap::CfgValue;
use rand::{thread_rng, RngCore};
use whoami::username;
use single_instance::SingleInstance;

// the modules defined and used in this application
mod task;
mod condition;
mod event;

mod common;
mod constants;
mod config;


// bring the registries in scope
use task::registry::TaskRegistry;
use condition::registry::ConditionRegistry;
use event::registry::EventRegistry;

use condition::bucket_cond::ExecutionBucket;

use common::logging::{log, init as log_init, LogType};
use config::*;
use constants::*;


lazy_static! {
    // the global task registry: all conditions will be associated to this
    static ref TASK_REGISTRY: TaskRegistry = TaskRegistry::new();

    // the global condition registry: the scheduler will use this
    static ref CONDITION_REGISTRY: ConditionRegistry = ConditionRegistry::new();

    // the global event registry: will be alive and active throughout eecution
    static ref EVENT_REGISTRY: EventRegistry = EventRegistry::new();

    // the execution bucket for the bucket/event based conditions
    static ref EXECUTION_BUCKET: ExecutionBucket = ExecutionBucket::new();

    // array of handles for threads that might be started by event listeners
    // WARNING: for now left alone, it would have to be synchronized and possibly
    //          deserves a registry on its own or to be implemented in the event
    //          registry itself in order to dynamically add or remove listeners;
    //          the main function cannot to this job on its own
    // static ref EVENT_HANDLES: Vec<JoinHandle<Result<bool, std::io::Error>>> = Vec::new();

    // single instance name
    static ref INSTANCE_GUID: String = format!("{APP_NAME}-{}-{APP_GUID}", username());

    // set this if the application must exit
    static ref APPLICATION_MUST_EXIT: RwLock<bool> = RwLock::new(false);

    // set this if the application must exit
    static ref APPLICATION_FORCE_EXIT: RwLock<bool> = RwLock::new(false);

    // set this if the application must exit
    static ref APPLICATION_IS_PAUSED: RwLock<bool> = RwLock::new(false);

    // types of conditions whose tick cannot be delayed
    static ref NO_DELAY_CONDITIONS: Vec<String> = vec![
        String::from("interval"),
        String::from("time"),
        String::from("idle"),
        ];

    // the buffered standard input for command line reads (no Mutex: already synchronized)
    static ref STDIN: Stdin = stdin();

}


// check whether an instance is already running, and return an error if so
fn check_single_instance(instance: &SingleInstance) -> std::io::Result<()> {
    if !instance.is_single() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            ERR_ALREADY_RUNNING,
        ));
    }

    Ok(())
}



// execute a (very basic but working) scheduler tick: the call to this function
// is executed in a separate thread; the function itself will spawn as many
// threads as there are conditions to check, so that the short-running ones can
// finish and get out of the way to allow execution of subsequent ticks; within
// the new thread the tick might wait for a random duration,
fn sched_tick(rand_millis_range: Option<u64>) -> std::io::Result<bool> {
    // skip if application is paused
    if *APPLICATION_IS_PAUSED.read().unwrap() {
        log(
            LogType::Trace,
            LOG_EMITTER_MAIN,
            LOG_ACTION_SCHEDULER_TICK,
            None,
            LOG_WHEN_PROC,
            LOG_STATUS_MSG,
            "application is paused: tick skipped",
        );
        return Ok(false);
    }

    for name in CONDITION_REGISTRY.condition_names().unwrap() {
        // go away if condition is busy
        if CONDITION_REGISTRY.condition_busy(&name) {
            log(
                LogType::Debug,
                LOG_EMITTER_MAIN,
                LOG_ACTION_SCHEDULER_TICK,
                None,
                LOG_WHEN_PROC,
                LOG_STATUS_MSG,
                &format!("condition {name} is busy: tick skipped"),
            );
            continue
        }
        // else...

        // create a new thread for each check: note that each thread will
        // attempt to lock the condition registry, thus wait for it to be
        // released by the previous owner
        std::thread::spawn(move || {
            if !NO_DELAY_CONDITIONS.contains(&CONDITION_REGISTRY.condition_type(&name).unwrap()) {
                if let Some(ms) = rand_millis_range {
                    let mut rng = thread_rng();
                    let rms = rng.next_u64() % ms;
                    let dur = std::time::Duration::from_millis(rms);
                    std::thread::sleep(dur);
                }
            }
            if let Ok(outcome) = CONDITION_REGISTRY.tick(&name) {
                match outcome {
                    Some(res) => {
                        if res {
                            log(
                                LogType::Trace,
                                LOG_EMITTER_MAIN,
                                LOG_ACTION_SCHEDULER_TICK,
                                None,
                                LOG_WHEN_PROC,
                                LOG_STATUS_MSG,
                                &format!("condition {name} tested (tasks executed)"),
                            );
                        } else {
                            log(
                                LogType::Trace,
                                LOG_EMITTER_MAIN,
                                LOG_ACTION_SCHEDULER_TICK,
                                None,
                                LOG_WHEN_PROC,
                                LOG_STATUS_MSG,
                                &format!("condition {name} tested (tasks executed unsuccessfully)"),
                            );
                        }
                    }
                    None => {
                        log(
                            LogType::Trace,
                            LOG_EMITTER_MAIN,
                            LOG_ACTION_SCHEDULER_TICK,
                            None,
                            LOG_WHEN_PROC,
                            LOG_STATUS_MSG,
                            &format!("condition {name} tested with no outcome (tasks not executed)"),
                        );
                    }
                }
            }
        });
    }

    Ok(true)
}



// this is similar to my usual exiterror
macro_rules! exit_if_fails {
    ( $quiet:expr, $might_fail:expr ) => {
        match $might_fail {
            Err(e) => {
                if !$quiet {
                    if cfg!(debug_assertions) {
                        eprintln!("{APP_NAME} error: {:?}", e);
                    } else {
                        eprintln!("{APP_NAME} error: {}", e.to_string());
                    }
                }
                std::process::exit(2);
            }
            Ok(value) => value
        }
    }
}



// reset the conditions whose names are provided in a vector of &str
fn reset_conditions(names: &[String]) -> std::io::Result<bool> {

    // WARNING: this overly simplistic version, which is temporary, will
    // wait for each provided condition to exit its possible busy state;
    // this means that if a busy condition is met in the list, all the
    // following conditions will have to wait for it to be freed in order
    // to receive their reset instruction, and thiis can make the process
    // of resetting many conditions very long, while also blocking the
    // command interpreter thread

    for name in names {
        if !CONDITION_REGISTRY.has_condition(name) {
            log(
                LogType::Error,
                LOG_EMITTER_MAIN,
                LOG_ACTION_RESET_CONDITIONS,
                None,
                LOG_WHEN_START,
                LOG_STATUS_ERR,
                &format!("cannot reset non existent condition: {name}"),
            );
        } else {
            log(
                LogType::Debug,
                LOG_EMITTER_MAIN,
                LOG_ACTION_RESET_CONDITIONS,
                None,
                LOG_WHEN_START,
                LOG_STATUS_OK,
                &format!("resetting condition {name}"),
            );
            match CONDITION_REGISTRY.reset_condition(name, true) {
                Ok(res) => {
                    if res {
                        log(
                            LogType::Info,
                            LOG_EMITTER_MAIN,
                            LOG_ACTION_RESET_CONDITIONS,
                            None,
                            LOG_WHEN_END,
                            LOG_STATUS_OK,
                            &format!("condition {name} has been reset"),
                        );
                    } else {
                        log(
                            LogType::Warn,
                            LOG_EMITTER_MAIN,
                            LOG_ACTION_RESET_CONDITIONS,
                            None,
                            LOG_WHEN_END,
                            LOG_STATUS_FAIL,
                            &format!("condition {name} could not be reset"),
                        );
                    }
                }
                Err(e) => {
                    log(
                        LogType::Error,
                        LOG_EMITTER_MAIN,
                        LOG_ACTION_RESET_CONDITIONS,
                        None,
                        LOG_WHEN_END,
                        LOG_STATUS_FAIL,
                        &format!("error while resetting condition {name}: {e}"),
                    );
                }
            }
        }
    }

    Ok(true)
}


// set the suspended state for a condition identified by its name
fn set_suspended_condition(name: &str, suspended: bool) -> std::io::Result<bool> {

    if !CONDITION_REGISTRY.has_condition(name) {
        log(
            LogType::Error,
            LOG_EMITTER_MAIN,
            LOG_ACTION_CONDITION_STATE,
            None,
            LOG_WHEN_START,
            LOG_STATUS_ERR,
            &format!("cannot set state for non existent condition: {name}"),
        );
    } else {
        log(
            LogType::Debug,
            LOG_EMITTER_MAIN,
            LOG_ACTION_CONDITION_STATE,
            None,
            LOG_WHEN_START,
            LOG_STATUS_OK,
            &format!("changing state of condition {name} to {}", {
                if suspended { "suspended" } else { "active" }
            }),
        );
        if suspended {
            match CONDITION_REGISTRY.suspend_condition(name, true) {
                Ok(res) => {
                    if res {
                        log(
                            LogType::Info,
                            LOG_EMITTER_MAIN,
                            LOG_ACTION_SUSPEND_CONDITION,
                            None,
                            LOG_WHEN_END,
                            LOG_STATUS_OK,
                            &format!("condition {name} has been suspended"),
                        );
                    } else {
                        log(
                            LogType::Warn,
                            LOG_EMITTER_MAIN,
                            LOG_ACTION_SUSPEND_CONDITION,
                            None,
                            LOG_WHEN_END,
                            LOG_STATUS_FAIL,
                            &format!("condition {name} already suspended"),
                        );
                    }
                }
                Err(e) => {
                    log(
                        LogType::Error,
                        LOG_EMITTER_MAIN,
                        LOG_ACTION_SUSPEND_CONDITION,
                        None,
                        LOG_WHEN_END,
                        LOG_STATUS_FAIL,
                        &format!("error while suspending condition {name}: {e}"),
                    );
                }
            }
        } else {
            match CONDITION_REGISTRY.resume_condition(name, true) {
                Ok(res) => {
                    if res {
                        // we also want to reset the condition after it has
                        // been resumed, otherwise conditions based upon time
                        // intervals might fire immediately; reset will always
                        // succeed, so this construct to build the right log
                        // message is only here for consistency
                        let info;
                        if let Ok(res_rst) = CONDITION_REGISTRY.reset_condition(name, true) {
                            info = if res_rst { "resumed and reset" } else { "resumed" };
                        } else {
                            info = "resumed";
                        }
                        log(
                            LogType::Info,
                            LOG_EMITTER_MAIN,
                            LOG_ACTION_RESUME_CONDITION,
                            None,
                            LOG_WHEN_END,
                            LOG_STATUS_OK,
                            &format!("condition {name} has been {info}"),
                        );
                    } else {
                        log(
                            LogType::Warn,
                            LOG_EMITTER_MAIN,
                            LOG_ACTION_RESUME_CONDITION,
                            None,
                            LOG_WHEN_END,
                            LOG_STATUS_FAIL,
                            &format!("condition {name} was not suspended"),
                        );
                    }
                }
                Err(e) => {
                    log(
                        LogType::Error,
                        LOG_EMITTER_MAIN,
                        LOG_ACTION_RESUME_CONDITION,
                        None,
                        LOG_WHEN_END,
                        LOG_STATUS_FAIL,
                        &format!("error while resuming condition {name}: {e}"),
                    );
                }
            }
        }
    }

    Ok(true)
}


// attempt to trigger an event
fn trigger_event(name: &str) -> std::io::Result<bool> {
    if let Some(triggerable) = EVENT_REGISTRY.event_triggerable(name) {
        if triggerable {
            log(
                LogType::Debug,
                LOG_EMITTER_MAIN,
                LOG_ACTION_EVENT_TRIGGER,
                None,
                LOG_WHEN_START,
                LOG_STATUS_OK,
                &format!("triggering event {name}"),
            );
            match EVENT_REGISTRY.trigger_event(name) {
                Ok(res) => {
                    if res {
                        log(
                            LogType::Info,
                            LOG_EMITTER_MAIN,
                            LOG_ACTION_EVENT_TRIGGER,
                            None,
                            LOG_WHEN_END,
                            LOG_STATUS_OK,
                            &format!("event {name} successfully triggered"),
                        );
                        Ok(true)
                    } else {
                        log(
                            LogType::Warn,
                            LOG_EMITTER_MAIN,
                            LOG_ACTION_EVENT_TRIGGER,
                            None,
                            LOG_WHEN_END,
                            LOG_STATUS_ERR,
                            &format!("event {name} could not be triggered or condition cannot fire"),
                        );
                        Ok(false)
                    }
                }
                Err(e) => {
                    log(
                        LogType::Error,
                        LOG_EMITTER_MAIN,
                        LOG_ACTION_EVENT_TRIGGER,
                        None,
                        LOG_WHEN_END,
                        LOG_STATUS_ERR,
                        &format!("error triggering event {name}: {e}"),
                    );
                    Ok(false)
                }
            }
        } else {
            log(
                LogType::Error,
                LOG_EMITTER_MAIN,
                LOG_ACTION_EVENT_TRIGGER,
                None,
                LOG_WHEN_END,
                LOG_STATUS_ERR,
                &format!("event {name} cannot be triggered"),
            );
            Ok(false)
        }
    } else {
        log(
            LogType::Error,
            LOG_EMITTER_MAIN,
            LOG_ACTION_EVENT_TRIGGER,
            None,
            LOG_WHEN_START,
            LOG_STATUS_ERR,
            &format!("cannot trigger non existent event: {name}"),
        );
        Ok(false)
    }

}


// the following is a separate thread that reads stdin and interprets commands
// passed to the application through it: it is the only thread that reads
// from the standard input, therefore no explicit synchronization
fn interpret_commands() -> std::io::Result<bool> {
    let mut buffer = String::new();
    let rest_time = Duration::from_millis(MAIN_STDIN_READ_WAIT_MILLISECONDS);
    let mut handle = STDIN.lock();

    while let Ok(_n) = handle.read_line(&mut buffer) {
        // note that `split_whitespace()` already trims its argument
        let v: Vec<&str> = buffer.split_whitespace().collect();
        if v.len() > 0 {
            let cmd = v[0];
            let args = &v[1..];  // should not panic, but there might be a cleaner way
            match cmd {
                "exit" | "quit" => {
                    if !args.is_empty() {
                        log(
                            LogType::Error,
                            LOG_EMITTER_MAIN,
                            LOG_ACTION_RUN_COMMAND,
                            None,
                            LOG_WHEN_PROC,
                            LOG_STATUS_ERR,
                            &format!("command `{cmd}` does not support arguments"),
                        );
                    } else {
                        log(
                            LogType::Warn,
                            LOG_EMITTER_MAIN,
                            LOG_ACTION_RUN_COMMAND,
                            None,
                            LOG_WHEN_PROC,
                            LOG_STATUS_MSG,
                            "exit request received: terminating application",
                        );
                        *APPLICATION_MUST_EXIT.write().unwrap() = true;
                    }
                }
                "kill" => {
                    if !args.is_empty() {
                        log(
                            LogType::Error,
                            LOG_EMITTER_MAIN,
                            LOG_ACTION_RUN_COMMAND,
                            None,
                            LOG_WHEN_PROC,
                            LOG_STATUS_ERR,
                            &format!("command `{cmd}` does not support arguments"),
                        );
                    } else {
                        log(
                            LogType::Warn,
                            LOG_EMITTER_MAIN,
                            LOG_ACTION_RUN_COMMAND,
                            None,
                            LOG_WHEN_PROC,
                            LOG_STATUS_MSG,
                            "kill request received: terminating application immediately",
                        );
                        *APPLICATION_MUST_EXIT.write().unwrap() = true;
                        *APPLICATION_FORCE_EXIT.write().unwrap() = true;
                    }
                }
                "pause" => {
                    if !args.is_empty() {
                        log(
                            LogType::Error,
                            LOG_EMITTER_MAIN,
                            LOG_ACTION_RUN_COMMAND,
                            None,
                            LOG_WHEN_PROC,
                            LOG_STATUS_ERR,
                            &format!("command `{cmd}` does not support arguments"),
                        );
                    } else if *APPLICATION_IS_PAUSED.read().unwrap() {
                        log(
                            LogType::Warn,
                            LOG_EMITTER_MAIN,
                            LOG_ACTION_RUN_COMMAND,
                            None,
                            LOG_WHEN_PROC,
                            LOG_STATUS_FAIL,
                            "ignoring pause request: scheduler already paused",
                        );
                    } else {
                        log(
                            LogType::Debug,
                            LOG_EMITTER_MAIN,
                            LOG_ACTION_RUN_COMMAND,
                            None,
                            LOG_WHEN_PROC,
                            LOG_STATUS_MSG,
                            "pausing scheduler ticks: conditions not checked until resume",
                        );
                        *APPLICATION_IS_PAUSED.write().unwrap() = true;
                    }
                }
                "resume" => {
                    if !args.is_empty() {
                        log(
                            LogType::Error,
                            LOG_EMITTER_MAIN,
                            LOG_ACTION_RUN_COMMAND,
                            None,
                            LOG_WHEN_PROC,
                            LOG_STATUS_ERR,
                            &format!("command `{cmd}` does not support arguments"),
                        );
                    } else if *APPLICATION_IS_PAUSED.read().unwrap() {
                        log(
                            LogType::Debug,
                            LOG_EMITTER_MAIN,
                            LOG_ACTION_RUN_COMMAND,
                            None,
                            LOG_WHEN_PROC,
                            LOG_STATUS_MSG,
                            "resuming scheduler ticks and condition checks",
                        );
                        // clear execution bucket because events may still have
                        // occurred and maybe the user wanted to also pause event
                        // based conditions (NOTE: this is questionable, since
                        // multiple insertions are debounced it is probably more
                        // correct to just obey instructions and verify conditions
                        // associated to these events: commented out for now)
                        // EXECUTION_BUCKET.clear();
                        *APPLICATION_IS_PAUSED.write().unwrap() = false;
                    } else {
                        log(
                            LogType::Warn,
                            LOG_EMITTER_MAIN,
                            LOG_ACTION_RUN_COMMAND,
                            None,
                            LOG_WHEN_PROC,
                            LOG_STATUS_FAIL,
                            "ignoring resume request: scheduler is not paused",
                        );
                    }
                }
                "reset_conditions" => {
                    if args.is_empty() {
                        log(
                            LogType::Trace,
                            LOG_EMITTER_MAIN,
                            LOG_ACTION_RUN_COMMAND,
                            None,
                            LOG_WHEN_PROC,
                            LOG_STATUS_MSG,
                            "no names provided: attempt to reset all conditions",
                        );
                        if let Some(v) = CONDITION_REGISTRY.condition_names() {
                            if !v.is_empty() {
                                let _ = reset_conditions(v.as_slice());
                            } else {
                                // this branch is never executed: when there are
                                // no conditions in the registry, the result is
                                // always `None`
                                log(
                                    LogType::Debug,
                                    LOG_EMITTER_MAIN,
                                    LOG_ACTION_RUN_COMMAND,
                                    None,
                                    LOG_WHEN_PROC,
                                    LOG_STATUS_MSG,
                                    "there are no conditions to reset",
                                );
                            }
                        } else {
                            log(
                                LogType::Debug,
                                LOG_EMITTER_MAIN,
                                LOG_ACTION_RUN_COMMAND,
                                None,
                                LOG_WHEN_PROC,
                                LOG_STATUS_MSG,
                                "no conditions found in registry for reset",
                            );
                        }
                    } else {
                        // NOTE: here `v` is shadowing the outer one, could use
                        // another name; however creating `v` here allows for
                        // moving it into the new thread without having problems
                        // concerning its lifetime
                        let mut v: Vec<String> = Vec::new();
                        for a in args {
                            v.push(String::from(*a));
                        }
                        log(
                            LogType::Debug,
                            LOG_EMITTER_MAIN,
                            LOG_ACTION_RUN_COMMAND,
                            None,
                            LOG_WHEN_PROC,
                            LOG_STATUS_MSG,
                            &format!("attempting to reset conditions: {}", v.join(", ")),
                        );
                        // the new thread is to avoid the command line to be
                        // unavailable while possibly waiting for busy conditions
                        // to be free before actually performing the requested
                        // action: the same holds for the other commands below
                        // that might be blocking when their arguments refer to
                        // busy items; the choice should be safe because the
                        // input commands thread has the same lifetime as the
                        // main thread, so it never ends unless the main thread
                        // is forcibly terminated - in which case all the spawned
                        // threads are terminated abruptly as well
                        thread::spawn(move || { let _ = reset_conditions(v.as_slice()); });
                    }
                }
                "suspend_condition" => {
                    if args.len() != 1 {
                        log(
                            LogType::Error,
                            LOG_EMITTER_MAIN,
                            LOG_ACTION_RUN_COMMAND,
                            None,
                            LOG_WHEN_PROC,
                            LOG_STATUS_FAIL,
                            "invalid number of arguments for command `suspend_condition`",
                        );
                    } else {
                        log(
                            LogType::Debug,
                            LOG_EMITTER_MAIN,
                            LOG_ACTION_RUN_COMMAND,
                            None,
                            LOG_WHEN_PROC,
                            LOG_STATUS_MSG,
                            &format!("attempting to suspend condition {}", args[0]),
                        );
                        // same considerations as above
                        let arg = args[0].to_string();
                        thread::spawn(move || { let _ = set_suspended_condition(&arg, true); });
                    }
                }
                "resume_condition" => {
                    if args.len() != 1 {
                        log(
                            LogType::Error,
                            LOG_EMITTER_MAIN,
                            LOG_ACTION_RUN_COMMAND,
                            None,
                            LOG_WHEN_PROC,
                            LOG_STATUS_FAIL,
                            "invalid number of arguments for command `resume_condition`",
                        );
                    } else {
                        log(
                            LogType::Debug,
                            LOG_EMITTER_MAIN,
                            LOG_ACTION_RUN_COMMAND,
                            None,
                            LOG_WHEN_PROC,
                            LOG_STATUS_MSG,
                            &format!("attempting to resume condition {}", args[0]),
                        );
                        // same considerations as above
                        // condition is freed and the command can be executed
                        let arg = args[0].to_string();
                        thread::spawn(move || { let _ = set_suspended_condition(&arg, false); });
                    }
                }
                "trigger" => {
                    if args.len() != 1 {
                        log(
                            LogType::Error,
                            LOG_EMITTER_MAIN,
                            LOG_ACTION_RUN_COMMAND,
                            None,
                            LOG_WHEN_PROC,
                            LOG_STATUS_FAIL,
                            "invalid number of arguments for command `trigger`",
                        );
                    } else {
                        log(
                            LogType::Debug,
                            LOG_EMITTER_MAIN,
                            LOG_ACTION_RUN_COMMAND,
                            None,
                            LOG_WHEN_PROC,
                            LOG_STATUS_MSG,
                            &format!("attempting to trigger event {}", args[0]),
                        );
                        // same considerations as above
                        let arg = args[0].to_string();
                        thread::spawn(move || { let _ = trigger_event(&arg); });
                    }
                }
                // ...

                "" => { /* do nothing here */ }
                t => {
                    log(
                        LogType::Error,
                        LOG_EMITTER_MAIN,
                        LOG_ACTION_RUN_COMMAND,
                        None,
                        LOG_WHEN_PROC,
                        LOG_STATUS_ERR,
                        &format!("unrecognized command: `{t}`"),
                    );
                }
            }

            // clear the buffer immediately after consuming the line
            buffer.clear();
        }
        thread::sleep(rest_time);
    }

    Ok(true)
}


// the following is another separate thread for event registry management
// NOTE: for now it is just a placeholder for the actual function
fn manage_event_registry() -> std::io::Result<bool> {
    let rest_time = Duration::from_millis(MAIN_EVENT_REGISTRY_MGMT_MILLISECONDS);

    loop {
        if *APPLICATION_MUST_EXIT.read().unwrap() {
            break;
        }
        thread::sleep(rest_time);
    }

    Ok(true)
}


// argument parsing and command execution: doc comments are used by clap
use clap::{Parser, ValueEnum};


/// A lightweight task scheduler and automation tool
#[derive(Parser)]
#[command(name=APP_NAME, version, about)]
struct Args {
    /// Suppress all output
    #[arg(short, long)]
    quiet: bool,

    /// Start in paused mode
    #[arg(short, long)]
    pause: bool,

    /// Check whether an instance is running
    #[arg(short = 'r', long)]
    check_running: bool,

    /// Specify the log file
    #[arg(short, long, value_name = "LOGFILE")]
    log: Option<String>,

    /// Specify the log level
    #[arg(
        short = 'L',
        long,
        value_name = "LEVEL",
        default_value_t = LogLevel::Warn,
        default_missing_value = "warn",
        value_enum,
    )]
    log_level: LogLevel,

    /// Append to an existing log file if found
    #[arg(short = 'a', long, requires = "log")]
    log_append: bool,

    /// No colors when logging (default when logging to file)
    #[arg(short = 'P', long, group = "logformat")]
    log_plain: bool,

    /// Use colors when logging (default, ignored when logging to file)
    #[arg(short = 'C', long, group = "logformat")]
    log_color: bool,

    /// Use JSON format for logging
    #[arg(short = 'J', long, group = "logformat")]
    log_json: bool,

    /// Path to configuration file
    #[arg(value_name = "CONFIG")]
    config: Option<String>,
}


// this is redundant but necessary for clap (the `type` alias does not work)
#[derive(ValueEnum, Copy, Clone, Debug, PartialEq, Eq)]
enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}



// entry point
fn main() {
    // parse arguments
    let args = Args::parse();

    // check that no other instance is running
    let instance = SingleInstance::new(&INSTANCE_GUID).unwrap();

    // if asked to, check for a running instance and exit with a 0 exit status
    // if an instance is already running, 1 if no instaance is running, and if
    // not in quiet mode print a brief message on stdout
    if args.check_running {
        let not_running = instance.is_single();
        if !args.quiet {
            println!(
                "{APP_NAME}: {}",
                if not_running {
                    "no running instances"
                } else {
                    "an instance is running"
                }
            );
        }
        if not_running {
            std::process::exit(1);
        } else {
            std::process::exit(0);
        }
    }

    exit_if_fails!(args.quiet, check_single_instance(&instance));
    
    // now check that the config file name has been provided
    if args.config.is_none() {
        eprintln!("{APP_NAME} error: configuration file not specified");
        std::process::exit(2);
    }
    let config = args.config.unwrap();

    // configure the logger
    let level = match args.log_level {
        LogLevel::Trace => LogType::Trace,
        LogLevel::Debug => LogType::Debug,
        LogLevel::Info => LogType::Info,
        LogLevel::Warn => LogType::Warn,
        LogLevel::Error => LogType::Error,
    };
    let log_file_name = args.log;
    exit_if_fails!(args.quiet,
        log_init(
            level,
            log_file_name,
            args.log_append,
            args.log_color,
            args.log_plain,
            args.log_json,
        ));

    // configure CTRL-C handler to just log and exit without error
    exit_if_fails!(args.quiet, ctrlc::set_handler(move || {
        log(
            LogType::Warn,
            LOG_EMITTER_MAIN,
            LOG_ACTION_MAIN_EXIT,
            None,
            LOG_WHEN_END,
            LOG_STATUS_MSG,
            "caught interruption request: terminating application",
        );
        *APPLICATION_MUST_EXIT.write().unwrap() = true;
    }));

    // write a banner to the log file, stating app name and version
    log(
        LogType::Info,
        LOG_EMITTER_MAIN,
        LOG_ACTION_MAIN_START,
        None,
        LOG_WHEN_START,
        LOG_STATUS_MSG,
        &format!("{APP_NAME} {APP_VERSION}"),
    );

    // read configuration, then extract the global parameters and configure items
    // NOTE: the `unwrap_or` part is actually pleonastic, as the missing values
    //       are set by `configure()` to exactly the values below. This will
    //       eventually use plain `unwrap()`.
    let configuration = exit_if_fails!(args.quiet, configure(&config));
    let scheduler_tick_seconds = *configuration
        .get("scheduler_tick_seconds")
        .unwrap_or(&CfgValue::from(DEFAULT_SCHEDULER_TICK_SECONDS))
        .as_int()
        .unwrap_or(&DEFAULT_SCHEDULER_TICK_SECONDS) as u64;
    let randomize_checks_within_ticks = *configuration
        .get("randomize_checks_within_ticks")
        .unwrap_or(&CfgValue::from(DEFAULT_RANDOMIZE_CHECKS_WITHIN_TICKS))
        .as_bool()
        .unwrap_or(&DEFAULT_RANDOMIZE_CHECKS_WITHIN_TICKS);

    // before actual configuration the event service manager provided by
    // the event registry must be started, so that all configured event
    // services can be enqueued for startup at the beginning; this could
    // actually take place also after the configuration step
    event::registry::EventRegistry::start_event_service_manager(&EVENT_REGISTRY);

    // configure items given the parsed configuration map
    let mut _handles: Vec<JoinHandle<Result<bool, std::io::Error>>> = Vec::new();
    exit_if_fails!(args.quiet, configure_items(
        &configuration,
        &TASK_REGISTRY,
        &CONDITION_REGISTRY,
        &EVENT_REGISTRY,
        &mut _handles,
        &EXECUTION_BUCKET,
        scheduler_tick_seconds,
    ));

    // first of all check whether the application is started in paused mode
    // and if so check the appropriate flag and emit an info log message
    if args.pause {
        log(
            LogType::Info,
            LOG_EMITTER_MAIN,
            LOG_ACTION_SCHEDULER_TICK,
            None,
            LOG_WHEN_PROC,
            LOG_STATUS_MSG,
            "starting in paused mode",
        );
        *APPLICATION_IS_PAUSED.write().unwrap() = true;
    }

    // add a thread for stdin interpreter (no args function thus no closure)
    _handles.push(thread::spawn(interpret_commands));

    // add a thread to handle event registry management (same as above)
    _handles.push(thread::spawn(manage_event_registry));

    // shortcut to spawn a tick in the background
    fn spawn_tick(rand_millis_range: Option<u64>) {
        std::thread::spawn(move || { let _ = sched_tick(rand_millis_range); });
    }

    // set up a very minimal scheduler, and pass the option to randomize
    // condition tests within ticks if the user made the choice to do so
    // via the configuration file
    let mut sched = Scheduler::new();
    let rand_millis_range = {
        if randomize_checks_within_ticks {
            Some(scheduler_tick_seconds * 1000)
        } else {
            None
        }
    };
    sched.every((scheduler_tick_seconds as u32).seconds()).run(
        move || { spawn_tick(rand_millis_range); }
    );

    // free_pending must be a fraction of scheduler tick interval (say 1/10)
    let free_pending = Duration::from_millis(scheduler_tick_seconds * 100);

    // the main loop mostly sleeps, just to wake up every `free_pending` msecs
    // and tell the scheduler to do its job checking conditions, check whether
    // the exit flags are set and, if this is the case, set up things to exit
    loop {
        sched.run_pending();
        thread::sleep(free_pending);
        if *APPLICATION_MUST_EXIT.read().unwrap() {
            if *APPLICATION_FORCE_EXIT.read().unwrap() {
                log(
                    LogType::Warn,
                    LOG_EMITTER_MAIN,
                    LOG_ACTION_MAIN_EXIT,
                    None,
                    LOG_WHEN_END,
                    LOG_STATUS_OK,
                    "application exiting: all activity will be forced to stop",
                );
                break;
            } else {
                log(
                    LogType::Info,
                    LOG_EMITTER_MAIN,
                    LOG_ACTION_MAIN_EXIT,
                    None,
                    LOG_WHEN_END,
                    LOG_STATUS_OK,
                    "application exiting: waiting for activity to finish",
                );
                // wait for all currently running conditions to finish their
                // tick: during this time no `sched.run_pending();` is run, to
                // ensure that no new tests or tasks are initiated again
                while let Some(c) = CONDITION_REGISTRY.conditions_busy().unwrap() {
                    if c > 0 {
                        thread::sleep(free_pending);
                    } else {
                        break;
                    }
                }
                break;
            }
        }
    }

    log(
        LogType::Info,
        LOG_EMITTER_MAIN,
        LOG_ACTION_MAIN_EXIT,
        None,
        LOG_WHEN_END,
        LOG_STATUS_OK,
        "application exit: main process terminating successfully",
    );
    std::process::exit(0);
}


// end.
