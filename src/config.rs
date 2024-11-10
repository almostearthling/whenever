//! pub config
//!
//! module to load the application configuration from a TOML file

use std::fs;
use cfgmap::{CfgValue, CfgMap};

use crate::common::logging::{log, LogType};
use crate::condition::bucket_cond::ExecutionBucket;
use crate::constants::*;

// bring the registries in scope
use crate::task::registry::TaskRegistry;
use crate::condition::registry::ConditionRegistry;
use crate::event::registry::EventRegistry;

use crate::task;
use crate::condition;
use crate::event;
use crate::event::base::Event;


// read the configuration from a string and build tasks and conditions
pub fn configure(config_file: &str) -> std::io::Result<CfgMap> {

    // helper to create a specific error
    fn _c_error_invalid_config(key: &str) -> std::io::Error {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("{ERR_INVALID_CONFIG_FILE}:{key}"),
        )
    }

    let mut config_map: CfgMap;     // to be initialized below

    match toml::from_str(fs::read_to_string(config_file)?.as_str()) {
        Ok(toml_text) => {
            config_map = CfgMap::from_toml(toml_text);
        }
        _ => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                ERR_INVALID_CONFIG_FILE,
            ));
        }
    }

    let cur_key = "scheduler_tick_seconds";
    let mut scheduler_tick_seconds = DEFAULT_SCHEDULER_TICK_SECONDS;
    if let Some(item) = config_map.get(cur_key) {
        if !item.is_int() {
            return Err(_c_error_invalid_config(cur_key));
        }
        scheduler_tick_seconds = *item.as_int().unwrap();
        if scheduler_tick_seconds < 1 {
            return Err(_c_error_invalid_config(cur_key));
        }
    }

    let cur_key = "randomize_checks_within_ticks";
    let mut randomize_checks_within_ticks = DEFAULT_RANDOMIZE_CHECKS_WITHIN_TICKS;
    if let Some(item) = config_map.get(cur_key) {
        if !item.is_int() {
            return Err(_c_error_invalid_config(cur_key));
        }
        randomize_checks_within_ticks = *item.as_bool().unwrap();
    }

    // ...

    // complete the global configuration map if any values were not present
    let _ = config_map.add(
        "scheduler_tick_seconds", CfgValue::from(scheduler_tick_seconds));
    let _ = config_map.add(
        "randomize_checks_within_ticks", CfgValue::from(randomize_checks_within_ticks));

    Ok(config_map)
}


// configure the tasks according to the provided configuration map
fn configure_tasks(
    cfgmap: &CfgMap,
    task_registry: &'static TaskRegistry,
) -> std::io::Result<()> {
    if let Some(task_map) = cfgmap.get("task") {
        if !task_map.is_list() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                ERR_INVALID_TASK_CONFIG,
            ));
        } else {
            for entry in task_map.as_list().unwrap() {
                if !entry.is_map() {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        ERR_INVALID_TASK_CONFIG,
                    ));
                } else if let Some(task_type) = entry.as_map().unwrap().get("type") {
                    let task_type = task_type.as_str().unwrap();
                    match task_type.as_str() {
                        "command" => {
                            let task = task::command_task::CommandTask::load_cfgmap(
                                entry.as_map().unwrap())?;
                            if !task_registry.add_task(Box::new(task))? {
                                return Err(std::io::Error::new(
                                    std::io::ErrorKind::InvalidData,
                                    ERR_TASKREG_TASK_NOT_ADDED,
                                ));
                            }
                        }
                        "lua" => {
                            let task = task::lua_task::LuaTask::load_cfgmap(
                                entry.as_map().unwrap())?;
                            if !task_registry.add_task(Box::new(task))? {
                                return Err(std::io::Error::new(
                                    std::io::ErrorKind::InvalidData,
                                    ERR_TASKREG_TASK_NOT_ADDED,
                                ));
                            }
                        }
                        // ...

                        _ => {
                            return Err(std::io::Error::new(
                                std::io::ErrorKind::InvalidData,
                                ERR_INVALID_TASK_TYPE,
                            ));
                        }
                    }
                } else {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        ERR_INVALID_TASK_CONFIG,
                    ));
                }
            }
        }
    }
    Ok(())
}


// configure the conditions according to the provided configuration map
fn configure_conditions(
    cfgmap: &CfgMap,
    cond_registry: &'static ConditionRegistry,
    task_registry: &'static TaskRegistry,
    bucket: &'static ExecutionBucket,
    tick_secs: u64,
) -> std::io::Result<()> {
    if let Some(condition_map) = cfgmap.get("condition") {
        if !condition_map.is_list() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                ERR_INVALID_COND_CONFIG,
            ));
        } else {
            for entry in condition_map.as_list().unwrap() {
                if !entry.is_map() {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        ERR_INVALID_COND_CONFIG,
                    ));
                } else if let Some(condition_type) = entry.as_map().unwrap().get("type") {
                    let condition_type = condition_type.as_str().unwrap();
                    match condition_type.as_str() {
                        "interval" => {
                            let condition = condition::interval_cond::IntervalCondition::load_cfgmap(
                                entry.as_map().unwrap(), task_registry)?;
                            if !cond_registry.add_condition(Box::new(condition))? {
                                return Err(std::io::Error::new(
                                    std::io::ErrorKind::InvalidData,
                                    ERR_CONDREG_COND_NOT_ADDED,
                                ));
                            }
                        }
                        "idle" => {
                            let condition = condition::idle_cond::IdleCondition::load_cfgmap(
                                entry.as_map().unwrap(), task_registry)?;
                            if !cond_registry.add_condition(Box::new(condition))? {
                                return Err(std::io::Error::new(
                                    std::io::ErrorKind::InvalidData,
                                    ERR_CONDREG_COND_NOT_ADDED,
                                ));
                            }
                        }
                        "time" => {
                            // this is peculiar because it requires extra initialization after loading from map
                            let mut condition = condition::time_cond::TimeCondition::load_cfgmap(
                                entry.as_map().unwrap(), task_registry)?;
                            let _ = condition.set_tick_duration(tick_secs)?;
                            if !cond_registry.add_condition(Box::new(condition))? {
                                return Err(std::io::Error::new(
                                    std::io::ErrorKind::InvalidData,
                                    ERR_CONDREG_COND_NOT_ADDED,
                                ));
                            }
                        }
                        "command" => {
                            let condition = condition::command_cond::CommandCondition::load_cfgmap(
                                entry.as_map().unwrap(), task_registry)?;
                            if !cond_registry.add_condition(Box::new(condition))? {
                                return Err(std::io::Error::new(
                                    std::io::ErrorKind::InvalidData,
                                    ERR_CONDREG_COND_NOT_ADDED,
                                ));
                            }
                        }
                        "lua" => {
                            let condition = condition::lua_cond::LuaCondition::load_cfgmap(
                                entry.as_map().unwrap(), task_registry)?;
                            if !cond_registry.add_condition(Box::new(condition))? {
                                return Err(std::io::Error::new(
                                    std::io::ErrorKind::InvalidData,
                                    ERR_CONDREG_COND_NOT_ADDED,
                                ));
                            }
                        }
                        "dbus" => {
                            let condition = condition::dbus_cond::DbusMethodCondition::load_cfgmap(
                                entry.as_map().unwrap(), task_registry)?;
                            if !cond_registry.add_condition(Box::new(condition))? {
                                return Err(std::io::Error::new(
                                    std::io::ErrorKind::InvalidData,
                                    ERR_CONDREG_COND_NOT_ADDED,
                                ));
                            }
                        }
                        "bucket" | "event" => {
                            // this is peculiar because it requires extra initialization after loading from map
                            let mut condition = condition::bucket_cond::BucketCondition::load_cfgmap(
                                entry.as_map().unwrap(), task_registry)?;
                            let _ = condition.set_execution_bucket(bucket)?;
                            if !cond_registry.add_condition(Box::new(condition))? {
                                return Err(std::io::Error::new(
                                    std::io::ErrorKind::InvalidData,
                                    ERR_CONDREG_COND_NOT_ADDED,
                                ));
                            }
                        }
                        // ...

                        _ => {
                            return Err(std::io::Error::new(
                                std::io::ErrorKind::InvalidData,
                                ERR_INVALID_COND_TYPE,
                            ));
                        }
                    }
                } else {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        ERR_INVALID_COND_CONFIG,
                    ));
                }
            }
        }
    }
    Ok(())
}


// configure the event according to the provided configuration map
fn configure_events(
    cfgmap: &CfgMap,
    event_registry: &'static EventRegistry,
    cond_registry: &'static ConditionRegistry,
    bucket: &'static ExecutionBucket,
) -> std::io::Result<()> {

    if let Some(event_map) = cfgmap.get("event") {
        if !event_map.is_list() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                ERR_INVALID_EVENT_CONFIG,
            ));
        } else {
            for entry in event_map.as_list().unwrap() {
                if !entry.is_map() {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        ERR_INVALID_EVENT_CONFIG,
                    ));
                } else if let Some(event_type) = entry.as_map().unwrap().get("type") {
                    let event_type = event_type.as_str().unwrap();
                    match event_type.as_str() {
                        "fschange" => {
                            let event = event::fschange_event::FilesystemChangeEvent::load_cfgmap(
                                entry.as_map().unwrap(), cond_registry, bucket)?;
                            let event_name = event.get_name();
                            if !event_registry.add_event(Box::new(event))? {
                                return Err(std::io::Error::new(
                                    std::io::ErrorKind::InvalidData,
                                    ERR_EVENTREG_EVENT_NOT_ADDED,
                                ));
                            } else if let Ok(_) = event_registry.listen_for(&event_name) {
                                log(
                                    LogType::Trace,
                                    LOG_EMITTER_CONFIGURATION,
                                    LOG_ACTION_MAIN_LISTENER,
                                    None,
                                    LOG_WHEN_INIT,
                                    LOG_STATUS_MSG,
                                    &format!("service installed for event {event_name} (dedicated thread)"),
                                )
                            } else {
                                return Err(std::io::Error::new(
                                    std::io::ErrorKind::InvalidData,
                                    ERR_EVENTREG_EVENT_NOT_ADDED,
                                ));
                            }
                        }
                        "dbus" => {
                            let event = event::dbus_event::DbusMessageEvent::load_cfgmap(
                                entry.as_map().unwrap(), cond_registry, bucket)?;
                            let event_name = event.get_name();
                            if !event_registry.add_event(Box::new(event))? {
                                return Err(std::io::Error::new(
                                    std::io::ErrorKind::InvalidData,
                                    ERR_EVENTREG_EVENT_NOT_ADDED,
                                ));
                            } else if let Ok(_) = event_registry.listen_for(&event_name) {
                                log(
                                    LogType::Trace,
                                    LOG_EMITTER_CONFIGURATION,
                                    LOG_ACTION_MAIN_LISTENER,
                                    None,
                                    LOG_WHEN_INIT,
                                    LOG_STATUS_MSG,
                                    &format!("service installed for event {event_name} (dedicated thread)"),
                                )
                            } else {
                                return Err(std::io::Error::new(
                                    std::io::ErrorKind::InvalidData,
                                    ERR_EVENTREG_EVENT_NOT_ADDED,
                                ));
                            }
                        }
                        "cli" => {
                            let event = event::manual_event::ManualCommandEvent::load_cfgmap(
                                entry.as_map().unwrap(), cond_registry, bucket)?;
                            let event_name = event.get_name();
                            if !event_registry.add_event(Box::new(event))? {
                                return Err(std::io::Error::new(
                                    std::io::ErrorKind::InvalidData,
                                    ERR_EVENTREG_EVENT_NOT_ADDED,
                                ));
                            } else if let Ok(_) = event_registry.listen_for(&event_name) {
                                    log(
                                        LogType::Trace,
                                        LOG_EMITTER_CONFIGURATION,
                                        LOG_ACTION_MAIN_LISTENER,
                                        None,
                                        LOG_WHEN_INIT,
                                        LOG_STATUS_MSG,
                                        &format!("service installed for event {event_name} (dedicated thread)"),
                                    )
                            } else {
                                return Err(std::io::Error::new(
                                    std::io::ErrorKind::InvalidData,
                                    ERR_EVENTREG_EVENT_NOT_ADDED,
                                ));
                            }
                        }
                        // ...

                        _ => {
                            return Err(std::io::Error::new(
                                std::io::ErrorKind::InvalidData,
                                ERR_INVALID_EVENT_TYPE,
                            ));
                        }
                    }
                } else {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        ERR_INVALID_EVENT_CONFIG,
                    ));
                }
            }
        }
    }

    Ok(())
}



/// Configure all items given a configuration map
pub fn configure_items(
    cfgmap: &CfgMap,
    task_registry: &'static TaskRegistry,
    cond_registry: &'static ConditionRegistry,
    event_registry: &'static EventRegistry,
    bucket: &'static ExecutionBucket,
    tick_secs: u64,
) -> std::io::Result<()> {
    configure_tasks(cfgmap, task_registry)?;
    configure_conditions(cfgmap, cond_registry, task_registry, bucket, tick_secs)?;
    configure_events(cfgmap, event_registry, cond_registry, bucket)?;
    Ok(())
}








// end.
