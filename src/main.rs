//! # whenever
//!
//! A lightweight multiplatform background job launcher based upon
//! verification of various types of conditions.

use rand::{Rng, rng};
use std::io::{BufRead, Stdin, stdin};
use std::sync::{Mutex, RwLock};
use std::thread;
use std::time::Duration;

use lazy_static::lazy_static;

use cfgmap::CfgValue;
use clokwerk::{Scheduler, TimeUnits};
use single_instance::SingleInstance;
use whoami::username;

// the modules defined and used in this application
mod condition;
mod event;
mod task;

mod cfghelp;
mod common;
mod config;
mod constants;

// bring the registries in scope
use condition::registry::ConditionRegistry;
use event::registry::EventRegistry;
use task::registry::TaskRegistry;

use condition::bucket_cond::ExecutionBucket;
use task::internal_task::set_command_runner;

use crate::common::wres::{Error, Kind, Result};
use common::logging::{LogType, init as log_init, log};
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

    // single instance name
    static ref INSTANCE_GUID: String = format!(
        "{APP_NAME}-{}-{APP_GUID}",
        { if let Ok(s) = username() { s } else { String::from(STR_UNKNOWN_VALUE) }},
    );

    // set this if the application must exit
    static ref APPLICATION_MUST_EXIT: RwLock<bool> = RwLock::new(false);

    // set this if the application must exit immediately
    static ref APPLICATION_FORCE_EXIT: RwLock<bool> = RwLock::new(false);

    // set this if the application is paused
    static ref APPLICATION_IS_PAUSED: RwLock<bool> = RwLock::new(false);

    // set this if the application is paused waiting for reconfiguration
    static ref APPLICATION_IS_RECONFIGURING: RwLock<bool> = RwLock::new(false);

    // this is to have the input command executor only run a command at a time
    static ref INPUT_COMMAND_LOCK: Mutex<()> = Mutex::new(());

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
fn check_single_instance(instance: &SingleInstance) -> Result<()> {
    if !instance.is_single() {
        return Err(Error::new(Kind::Forbidden, ERR_ALREADY_RUNNING));
    }

    Ok(())
}

// execute a (very basic but working) scheduler tick: the call to this function
// is executed in a separate thread; the function itself will spawn as many
// threads as there are conditions to check, so that the short-running ones can
// finish and get out of the way to allow execution of subsequent ticks; within
// the new thread the tick might wait for a random duration,
fn sched_tick(rand_millis_range: Option<u64>) -> Result<bool> {
    // log whether or not there are any busy conditions
    let busy_conds = CONDITION_REGISTRY.conditions_busy().unwrap();
    if let Some(busy_conds) = busy_conds {
        if busy_conds > 0 {
            log(
                LogType::Trace,
                LOG_EMITTER_MAIN,
                LOG_ACTION_SCHEDULER_TICK,
                None,
                LOG_WHEN_BUSY,
                LOG_STATUS_YES,
                &format!("busy conditions reported (total: {busy_conds})"),
            );
        } else {
            log(
                LogType::Trace,
                LOG_EMITTER_MAIN,
                LOG_ACTION_SCHEDULER_TICK,
                None,
                LOG_WHEN_BUSY,
                LOG_STATUS_NO,
                "no busy conditions present (total: 0)",
            );
        }
    }

    // skip if application has been intentionally paused
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
    // also skip if application has been paused for reconfiguration
    if *APPLICATION_IS_RECONFIGURING.read().unwrap() {
        log(
            LogType::Trace,
            LOG_EMITTER_MAIN,
            LOG_ACTION_SCHEDULER_TICK,
            None,
            LOG_WHEN_PROC,
            LOG_STATUS_MSG,
            "application is reconfiguring: tick skipped",
        );
        return Ok(false);
    }

    for name in CONDITION_REGISTRY.condition_names()?.unwrap() {
        // go away if condition is busy
        if CONDITION_REGISTRY.condition_busy(&name)? {
            log(
                LogType::Debug,
                LOG_EMITTER_MAIN,
                LOG_ACTION_SCHEDULER_TICK,
                None,
                LOG_WHEN_PROC,
                LOG_STATUS_MSG,
                &format!("condition {name} is busy: tick skipped"),
            );
            continue;
        }
        // else...

        // create a new thread for each check: note that each thread will
        // attempt to lock the condition registry, thus wait for it to be
        // released by the previous owner
        std::thread::spawn(move || {
            let cond_type = CONDITION_REGISTRY.condition_type(&name);
            if cond_type.is_ok() {
                if let Some(cond_type) = cond_type.unwrap() {
                    if !NO_DELAY_CONDITIONS.contains(&cond_type) {
                        if let Some(ms) = rand_millis_range {
                            let mut rng = rng();
                            let rms = rng.next_u64() % ms;
                            let dur = std::time::Duration::from_millis(rms);
                            std::thread::sleep(dur);
                        }
                    }
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
                            &format!("condition {name} tested (tasks not executed)"),
                        );
                    }
                }
            } else {
                log(
                    LogType::Debug,
                    LOG_EMITTER_MAIN,
                    LOG_ACTION_SCHEDULER_TICK,
                    None,
                    LOG_WHEN_PROC,
                    LOG_STATUS_FAIL,
                    &format!("condition {name} could not be tested"),
                );
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
            Ok(value) => value,
        }
    };
}

// reset the conditions whose names are provided in a vector of &str
fn reset_conditions(names: &[String]) -> Result<bool> {
    for name in names {
        if !CONDITION_REGISTRY.has_condition(name)? {
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
                LogType::Trace,
                LOG_EMITTER_MAIN,
                LOG_ACTION_RESET_CONDITIONS,
                None,
                LOG_WHEN_START,
                LOG_STATUS_OK,
                &format!("resetting condition {name}"),
            );
            // since a reset request will block until a busy condition
            // releases its lock, it is better to queue a reset request
            // which will be handled by the registry
            if CONDITION_REGISTRY.reset_condition(name, false).is_err() {
                if CONDITION_REGISTRY.queue_reset_condition(name).is_ok() {
                    log(
                        LogType::Info,
                        LOG_EMITTER_MAIN,
                        LOG_ACTION_RESET_CONDITIONS,
                        None,
                        LOG_WHEN_PROC,
                        LOG_STATUS_OK,
                        &format!("condition {name} queued for reset"),
                    );
                } else {
                    log(
                        LogType::Warn,
                        LOG_EMITTER_MAIN,
                        LOG_ACTION_RESET_CONDITIONS,
                        None,
                        LOG_WHEN_PROC,
                        LOG_STATUS_FAIL,
                        &format!("condition {name} could not be queued for reset"),
                    );
                }
            } else {
                log(
                    LogType::Info,
                    LOG_EMITTER_MAIN,
                    LOG_ACTION_RESET_CONDITIONS,
                    None,
                    LOG_WHEN_END,
                    LOG_STATUS_OK,
                    &format!("condition {name} successfully reset"),
                );
            }
        }
    }

    Ok(true)
}

// set the suspended state for a condition identified by its name
fn set_suspended_condition(name: &str, suspended: bool) -> Result<bool> {
    if !CONDITION_REGISTRY.has_condition(name)? {
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
            // suspension is like reset: while a condition is busy it is
            // better not to block waiting for it to be released, and just
            // queue a suspension request
            if CONDITION_REGISTRY.suspend_condition(name, false).is_err() {
                if CONDITION_REGISTRY.queue_suspend_condition(name).is_ok() {
                    log(
                        LogType::Info,
                        LOG_EMITTER_MAIN,
                        LOG_ACTION_SUSPEND_CONDITION,
                        None,
                        LOG_WHEN_PROC,
                        LOG_STATUS_OK,
                        &format!("condition {name} queued to be suspended"),
                    );
                } else {
                    log(
                        LogType::Warn,
                        LOG_EMITTER_MAIN,
                        LOG_ACTION_SUSPEND_CONDITION,
                        None,
                        LOG_WHEN_PROC,
                        LOG_STATUS_FAIL,
                        &format!("condition {name} could not be queued to be suspended"),
                    );
                }
            } else {
                log(
                    LogType::Info,
                    LOG_EMITTER_MAIN,
                    LOG_ACTION_SUSPEND_CONDITION,
                    None,
                    LOG_WHEN_END,
                    LOG_STATUS_OK,
                    &format!("condition {name} successfully suspended"),
                );
            }
        } else {
            // the resume case is different, because in most cases the
            // condition is suspended and can be directly resumed, while if
            // not an error is returned and logged
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
                            info = if res_rst {
                                "resumed and reset"
                            } else {
                                "resumed"
                            };
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

// attempt to reconfigure the application using the provided config file name
fn reconfigure(config_file: &str) -> Result<()> {
    if let Err(e) = check_configuration(config_file) {
        log(
            LogType::Error,
            LOG_EMITTER_MAIN,
            LOG_ACTION_RECONFIGURE,
            None,
            LOG_WHEN_START,
            LOG_STATUS_FAIL,
            &format!("cannot use invalid configuration file `{config_file}`: {e}"),
        );
        return Err(e);
    }

    *APPLICATION_IS_RECONFIGURING.write().unwrap() = true;
    let res = reconfigure_globals(config_file);
    *APPLICATION_IS_RECONFIGURING.write().unwrap() = false;
    match res {
        Ok(config) => {
            let scheduler_tick_seconds = *config
                .get("scheduler_tick_seconds")
                .unwrap_or(&CfgValue::from(DEFAULT_SCHEDULER_TICK_SECONDS))
                .as_int()
                .unwrap_or(&DEFAULT_SCHEDULER_TICK_SECONDS)
                as u64;
            *APPLICATION_IS_RECONFIGURING.write().unwrap() = true;
            let res = reconfigure_items(
                &config,
                &TASK_REGISTRY,
                &CONDITION_REGISTRY,
                &EVENT_REGISTRY,
                &EXECUTION_BUCKET,
                scheduler_tick_seconds,
            );
            *APPLICATION_IS_RECONFIGURING.write().unwrap() = false;
            match res {
                Ok(_) => {
                    log(
                        LogType::Info,
                        LOG_EMITTER_MAIN,
                        LOG_ACTION_RECONFIGURE,
                        None,
                        LOG_WHEN_END,
                        LOG_STATUS_OK,
                        &format!("new configuration loaded from file `{config_file}`"),
                    );
                }
                Err(e) => {
                    log(
                        LogType::Error,
                        LOG_EMITTER_MAIN,
                        LOG_ACTION_RECONFIGURE,
                        None,
                        LOG_WHEN_END,
                        LOG_STATUS_FAIL,
                        &format!("errors found in configuration file `{config_file}`: {e}"),
                    );
                    return Err(e);
                }
            }
        }
        Err(e) => {
            log(
                LogType::Error,
                LOG_EMITTER_MAIN,
                LOG_ACTION_RECONFIGURE,
                None,
                LOG_WHEN_START,
                LOG_STATUS_FAIL,
                &format!("cannot load configuration file `{config_file}`: {e}"),
            );
            return Err(e);
        }
    }

    Ok(())
}

// attempt to trigger an event
fn trigger_event(name: &str) -> Result<bool> {
    if let Some(triggerable) = EVENT_REGISTRY.event_triggerable(name)? {
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
                            &format!(
                                "event {name} could not be triggered or condition cannot fire"
                            ),
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

// this function actually interprets and runs a command, passed as a string
pub fn run_command(line: &str) -> Result<bool> {
    // first of all, lock the command execution feature to avoid overlaps
    let _l = INPUT_COMMAND_LOCK.lock()?;

    let buffer_save = String::from(line);
    let v: Vec<&str> = line.split_whitespace().collect();
    if !v.is_empty() {
        let cmd = v[0];
        let args = &v[1..]; // should not panic, but there might be a cleaner way
        match cmd {
            "exit" | "quit" => {
                if !args.is_empty() {
                    let msg = format!("command `{cmd}` does not support arguments");
                    log(
                        LogType::Error,
                        LOG_EMITTER_MAIN,
                        LOG_ACTION_RUN_COMMAND,
                        None,
                        LOG_WHEN_PROC,
                        LOG_STATUS_ERR,
                        &msg,
                    );
                    Err(Error::new(Kind::Invalid, &msg))
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
                    Ok(true)
                }
            }
            "kill" => {
                if !args.is_empty() {
                    let msg = format!("command `{cmd}` does not support arguments");
                    log(
                        LogType::Error,
                        LOG_EMITTER_MAIN,
                        LOG_ACTION_RUN_COMMAND,
                        None,
                        LOG_WHEN_PROC,
                        LOG_STATUS_ERR,
                        &msg,
                    );
                    Err(Error::new(Kind::Invalid, &msg))
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
                    Ok(true)
                }
            }
            "pause" => {
                if !args.is_empty() {
                    let msg = format!("command `{cmd}` does not support arguments");
                    log(
                        LogType::Error,
                        LOG_EMITTER_MAIN,
                        LOG_ACTION_RUN_COMMAND,
                        None,
                        LOG_WHEN_PROC,
                        LOG_STATUS_ERR,
                        &msg,
                    );
                    Err(Error::new(Kind::Invalid, &msg))
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
                    Ok(false)
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
                    // this log line is for wrappers, to set a possible pause
                    // UI element (for instance: tray icon) on pause
                    log(
                        LogType::Trace,
                        LOG_EMITTER_MAIN,
                        LOG_ACTION_RUN_COMMAND,
                        None,
                        LOG_WHEN_PAUSE,
                        LOG_STATUS_YES,
                        "scheduler paused",
                    );
                    Ok(true)
                }
            }
            "resume" => {
                if !args.is_empty() {
                    let msg = format!("command `{cmd}` does not support arguments");
                    log(
                        LogType::Error,
                        LOG_EMITTER_MAIN,
                        LOG_ACTION_RUN_COMMAND,
                        None,
                        LOG_WHEN_PROC,
                        LOG_STATUS_ERR,
                        &format!("command `{cmd}` does not support arguments"),
                    );
                    Err(Error::new(Kind::Invalid, &msg))
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
                    // this log line is for wrappers, to reset a possible pause
                    // UI element (for instance: tray icon) on resume
                    log(
                        LogType::Trace,
                        LOG_EMITTER_MAIN,
                        LOG_ACTION_RUN_COMMAND,
                        None,
                        LOG_WHEN_PAUSE,
                        LOG_STATUS_NO,
                        "scheduler resumed",
                    );
                    Ok(true)
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
                    Ok(false)
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
                    if let Some(v) = CONDITION_REGISTRY.condition_names()? {
                        if !v.is_empty() {
                            let _ = reset_conditions(v.as_slice());
                            Ok(true)
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
                            Ok(false)
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
                        Ok(false)
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
                    thread::spawn(move || {
                        let _ = reset_conditions(v.as_slice());
                    });
                    Ok(true)
                }
            }
            "suspend_condition" => {
                if args.len() != 1 {
                    let msg = "invalid number of arguments for command `suspend_condition`";
                    log(
                        LogType::Error,
                        LOG_EMITTER_MAIN,
                        LOG_ACTION_RUN_COMMAND,
                        None,
                        LOG_WHEN_PROC,
                        LOG_STATUS_FAIL,
                        msg,
                    );
                    Err(Error::new(Kind::Invalid, msg))
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
                    thread::spawn(move || {
                        let _ = set_suspended_condition(&arg, true);
                    });
                    Ok(true)
                }
            }
            "resume_condition" => {
                if args.len() != 1 {
                    let msg = "invalid number of arguments for command `resume_condition`";
                    log(
                        LogType::Error,
                        LOG_EMITTER_MAIN,
                        LOG_ACTION_RUN_COMMAND,
                        None,
                        LOG_WHEN_PROC,
                        LOG_STATUS_FAIL,
                        msg,
                    );
                    Err(Error::new(Kind::Invalid, msg))
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
                    thread::spawn(move || {
                        let _ = set_suspended_condition(&arg, false);
                    });
                    Ok(true)
                }
            }
            "configure" => {
                // in this case take all the string after the first
                // space character in the command line, because the
                // filename might contain spaces, and in case of relative
                // paths even the first characters could be spaces:
                // "configure_".len() == 10
                let (_, fname) = buffer_save.split_at(10);
                let fname = String::from(fname.to_string().trim());
                log(
                    LogType::Debug,
                    LOG_EMITTER_MAIN,
                    LOG_ACTION_RUN_COMMAND,
                    None,
                    LOG_WHEN_PROC,
                    LOG_STATUS_MSG,
                    &format!("attempting to reconfigure using configuration file `{fname}`"),
                );
                // same considerations as above
                thread::spawn(move || {
                    let _ = reconfigure(&fname);
                });
                Ok(true)
            }
            "trigger" => {
                if args.len() != 1 {
                    let msg = "invalid number of arguments for command `trigger`";
                    log(
                        LogType::Error,
                        LOG_EMITTER_MAIN,
                        LOG_ACTION_RUN_COMMAND,
                        None,
                        LOG_WHEN_PROC,
                        LOG_STATUS_FAIL,
                        msg,
                    );
                    Err(Error::new(Kind::Invalid, msg))
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
                    thread::spawn(move || {
                        let _ = trigger_event(&arg);
                    });
                    Ok(true)
                }
            }
            // ...
            "" => {
                /* do nothing here */
                Ok(false)
            }
            t => {
                let msg = &format!("unrecognized command: `{t}`");
                log(
                    LogType::Error,
                    LOG_EMITTER_MAIN,
                    LOG_ACTION_RUN_COMMAND,
                    None,
                    LOG_WHEN_PROC,
                    LOG_STATUS_ERR,
                    msg,
                );
                Err(Error::new(Kind::Unsupported, msg))
            }
        }
    } else {
        let msg = "empty command line";
        log(
            LogType::Error,
            LOG_EMITTER_MAIN,
            LOG_ACTION_RUN_COMMAND,
            None,
            LOG_WHEN_PROC,
            LOG_STATUS_ERR,
            msg,
        );
        Err(Error::new(Kind::Invalid, msg))
    }
}

// the following is a separate thread that reads stdin and interprets commands
// passed to the application through it: it is the only thread that reads
// from the standard input, therefore no explicit synchronization
fn command_loop() -> Result<bool> {
    let mut buffer = String::new();
    let rest_time = Duration::from_millis(MAIN_STDIN_READ_WAIT_MILLISECONDS);
    let mut handle = STDIN.lock();

    while let Ok(_n) = handle.read_line(&mut buffer) {
        // we will decide what to do with this, for now ignore it and just log
        let _res = run_command(&buffer);

        // clear the buffer immediately after consuming the line
        buffer.clear();
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

    /// Provide the list of available optional features
    #[arg(short = 'O', long)]
    options: bool,

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

    if args.options {
        let options = [
            #[cfg(feature = "dbus")]
            "dbus",
            #[cfg(windows)]
            #[cfg(feature = "wmi")]
            "wmi",
            #[cfg(feature = "lua_unsafe")]
            "lua_unsafe",
        ];
        println!("options: {}", options.join(" "));
        std::process::exit(0);
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
    exit_if_fails!(
        args.quiet,
        log_init(
            level,
            log_file_name,
            args.log_append,
            args.log_color,
            args.log_plain,
            args.log_json,
        )
    );

    // configure CTRL-C handler to just log and exit without error
    exit_if_fails!(
        args.quiet,
        ctrlc::set_handler(move || {
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
        })
    );

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

    // check configuration
    exit_if_fails!(args.quiet, check_configuration(&config));

    // read configuration, then extract the global parameters and configure items
    // NOTE: the `unwrap_or` part is actually pleonastic, as the missing values
    //       are set by `configure()` to exactly the values below. This will
    //       eventually use plain `unwrap()`.
    let configuration = exit_if_fails!(args.quiet, configure_globals(&config));
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

    // set the unique command runner for internal command based tasks
    exit_if_fails!(args.quiet, set_command_runner(run_command));

    // configure items given the parsed configuration map
    exit_if_fails!(
        args.quiet,
        configure_items(
            &configuration,
            &TASK_REGISTRY,
            &CONDITION_REGISTRY,
            &EVENT_REGISTRY,
            &EXECUTION_BUCKET,
            scheduler_tick_seconds,
        )
    );

    // first of all check whether the application is started in paused mode
    // and if so check the appropriate flag and emit an info log message
    if args.pause {
        log(
            LogType::Info,
            LOG_EMITTER_MAIN,
            LOG_ACTION_MAIN_START,
            None,
            LOG_WHEN_PROC,
            LOG_STATUS_MSG,
            "starting in paused mode",
        );
        *APPLICATION_IS_PAUSED.write().unwrap() = true;
        log(
            LogType::Trace,
            LOG_EMITTER_MAIN,
            LOG_ACTION_MAIN_START,
            None,
            LOG_WHEN_PAUSE,
            LOG_STATUS_YES,
            "scheduler paused",
        );
    }

    // add a thread for stdin interpreter (no args function thus no closure)
    // this thread can be abruptly killed without worrying, so it is not added
    // to the threads to wait for before leaving
    let _ = thread::spawn(command_loop);

    // shortcut to spawn a tick in the background
    fn spawn_tick(rand_millis_range: Option<u64>) {
        std::thread::spawn(move || {
            let _ = sched_tick(rand_millis_range);
        });
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
    sched
        .every((scheduler_tick_seconds as u32).seconds())
        .run(move || {
            spawn_tick(rand_millis_range);
        });

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
                std::process::exit(1);
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

                // stop the event listener
                if let Err(e) = EVENT_REGISTRY.stop_event_listener() {
                    log(
                        LogType::Debug,
                        LOG_EMITTER_MAIN,
                        LOG_ACTION_MAIN_EXIT,
                        None,
                        LOG_WHEN_END,
                        LOG_STATUS_FAIL,
                        &format!("error stopping event listener: {e}"),
                    );
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
