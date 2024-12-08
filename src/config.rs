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
use crate::task::base::Task;
use crate::condition::base::Condition;
use crate::event::base::Event;

use crate::cfghelp::*;


// helper to create a specific error
// fn _c_error_invalid_config(key: &str) -> std::io::Error {
//     std::io::Error::new(
//         std::io::ErrorKind::InvalidInput,
//         format!("{ERR_INVALID_CONFIG_FILE}:{key}"),
//     )
// }


/// Check the configuration from a string
pub fn check_configuration(config_file: &str) -> std::io::Result<()> {

    let config_map: CfgMap;     // to be initialized below

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

    // globals
    cfg_int_check_above_eq(&config_map, "scheduler_tick_seconds", 1)?;
    cfg_bool(&config_map, "randomize_checks_within_ticks")?;

    // check tasks and build a list of names to check conditions against
    let mut task_list: Vec<String> = Vec::new();
    if let Some(item_map) = config_map.get("task") {
        if !item_map.is_list() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                ERR_INVALID_TASK_CONFIG,
            ));
        } else {
            for entry in item_map.as_list().unwrap() {
                if !entry.is_map() {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        ERR_INVALID_TASK_CONFIG,
                    ));
                } else if let Some(item_type) = entry.as_map().unwrap().get("type") {
                    let item_type = item_type.as_str().unwrap();
                    let name;
                    match item_type.as_str() {
                        "command" => {
                            name = task::command_task::CommandTask::check_cfgmap(&entry.as_map().unwrap())?;
                        }
                        "lua" => {
                            name = task::lua_task::LuaTask::check_cfgmap(&entry.as_map().unwrap())?;
                        }
                        // ...

                        _ => {
                            return Err(std::io::Error::new(
                                std::io::ErrorKind::InvalidData,
                                ERR_INVALID_TASK_TYPE,
                            ));
                        }
                    };
                    task_list.push(name);
                } else {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        ERR_INVALID_TASK_CONFIG,
                    ));
                }
            }
        }
    }

    // check conditions and build a list of names to check events against
    let mut condition_list: Vec<String> = Vec::new();
    if let Some(task_map) = config_map.get("condition") {
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
                } else if let Some(item_type) = entry.as_map().unwrap().get("type") {
                    let task_list = task_list.iter().map(|x| x.as_str()).collect();
                    let item_type = item_type.as_str().unwrap();
                    let name;
                    match item_type.as_str() {
                        "interval" => {
                            name = condition::interval_cond::IntervalCondition::check_cfgmap(&entry.as_map().unwrap(), &task_list)?;
                        }
                        "idle" => {
                            name = condition::idle_cond::IdleCondition::check_cfgmap(&entry.as_map().unwrap(), &task_list)?;
                        }
                        "time" => {
                            name = condition::time_cond::TimeCondition::check_cfgmap(&entry.as_map().unwrap(), &task_list)?;
                        }
                        "command" => {
                            name = condition::command_cond::CommandCondition::check_cfgmap(&entry.as_map().unwrap(), &task_list)?;
                        }
                        "lua" => {
                            name = condition::lua_cond::LuaCondition::check_cfgmap(&entry.as_map().unwrap(), &task_list)?;
                        }
                        "dbus" => {
                            name = condition::dbus_cond::DbusMethodCondition::check_cfgmap(&entry.as_map().unwrap(), &task_list)?;
                        }
                        "bucket" | "event" => {
                            name = condition::bucket_cond::BucketCondition::check_cfgmap(&entry.as_map().unwrap(), &task_list)?;
                        }
                        // ...

                        _ => {
                            return Err(std::io::Error::new(
                                std::io::ErrorKind::InvalidData,
                                ERR_INVALID_TASK_TYPE,
                            ));
                        }
                    };
                    condition_list.push(name);
                } else {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        ERR_INVALID_TASK_CONFIG,
                    ));
                }
            }
        }
    }

    // check events
    let mut event_list: Vec<String> = Vec::new();
    if let Some(item_map) = config_map.get("event") {
        if !item_map.is_list() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                ERR_INVALID_TASK_CONFIG,
            ));
        } else {
            for entry in item_map.as_list().unwrap() {
                if !entry.is_map() {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        ERR_INVALID_TASK_CONFIG,
                    ));
                } else if let Some(item_type) = entry.as_map().unwrap().get("type") {
                    let condition_list = condition_list.iter().map(|x| x.as_str()).collect();
                    let item_type = item_type.as_str().unwrap();
                    let name;
                    match item_type.as_str() {
                        "fschange" => {
                            name = event::fschange_event::FilesystemChangeEvent::check_cfgmap(&entry.as_map().unwrap(), &condition_list)?;
                        }
                        "dbus" => {
                            name = event::dbus_event::DbusMessageEvent::check_cfgmap(&entry.as_map().unwrap(), &condition_list)?;
                        }
                        "cli" => {
                            name = event::manual_event::ManualCommandEvent::check_cfgmap(&entry.as_map().unwrap(), &condition_list)?;
                        }
                        // ...

                        _ => {
                            return Err(std::io::Error::new(
                                std::io::ErrorKind::InvalidData,
                                ERR_INVALID_TASK_TYPE,
                            ));
                        }
                    };
                    event_list.push(name);
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


/// Read the configuration from a string and retrieve globals
pub fn configure_globals(config_file: &str) -> std::io::Result<CfgMap> {

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
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("{ERR_INVALID_CONFIG_FILE}:{cur_key}"),
            ));
        }
        scheduler_tick_seconds = *item.as_int().unwrap();
        if scheduler_tick_seconds < 1 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("{ERR_INVALID_CONFIG_FILE}:{cur_key}"),
            ));
        }
    }

    let cur_key = "randomize_checks_within_ticks";
    let mut randomize_checks_within_ticks = DEFAULT_RANDOMIZE_CHECKS_WITHIN_TICKS;
    if let Some(item) = config_map.get(cur_key) {
        if !item.is_int() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("{ERR_INVALID_CONFIG_FILE}:{cur_key}"),
            ));
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


/// Read the configuration from a string and retrieve globals
pub fn reconfigure_globals(config_file: &str) -> std::io::Result<CfgMap> {

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
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("{ERR_INVALID_CONFIG_FILE}:{cur_key}"),
            ));
        }
        scheduler_tick_seconds = *item.as_int().unwrap();
        if scheduler_tick_seconds < 1 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("{ERR_INVALID_CONFIG_FILE}:{cur_key}"),
            ));
        }
    }

    let cur_key = "randomize_checks_within_ticks";
    let mut randomize_checks_within_ticks = DEFAULT_RANDOMIZE_CHECKS_WITHIN_TICKS;
    if let Some(item) = config_map.get(cur_key) {
        if !item.is_int() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("{ERR_INVALID_CONFIG_FILE}:{cur_key}"),
            ));
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



// configure tasks according to the provided configuration map
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


// reconfigure tasks according to the provided configuration map
fn reconfigure_tasks(
    cfgmap: &CfgMap,
    task_registry: &'static TaskRegistry,
) -> std::io::Result<()> {

    let mut to_remove: Vec<String> = Vec::new();
    let e = task_registry.task_names();
    if e.is_some() {
        to_remove = e.unwrap().clone();
    }

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
                            let task_name = task.get_name();
                            if !task_registry.has_task(&task_name) || !task_registry.has_task_eq(&task) {
                                if !task_registry.dynamic_add_or_replace_task(Box::new(task))? {
                                    return Err(std::io::Error::new(
                                        std::io::ErrorKind::InvalidData,
                                        ERR_TASKREG_TASK_NOT_ADDED,
                                    ));
                                }
                            } else {
                                log(
                                    LogType::Trace,
                                    LOG_EMITTER_CONFIGURATION,
                                    LOG_ACTION_RECONFIGURE,
                                    None,
                                    LOG_WHEN_PROC,
                                    LOG_STATUS_MSG,
                                    &format!("not reconfiguring task {task_name}: no change detected"),
                                )
                            }
                            if to_remove.contains(&task_name) {
                                to_remove.swap_remove(to_remove.iter().position(|x| task_name == *x).unwrap());
                            }
                        }
                        "lua" => {
                            let task = task::lua_task::LuaTask::load_cfgmap(
                                entry.as_map().unwrap())?;
                            let task_name = task.get_name();
                            if !task_registry.has_task(&task_name) || !task_registry.has_task_eq(&task) {
                                if !task_registry.dynamic_add_or_replace_task(Box::new(task))? {
                                    return Err(std::io::Error::new(
                                        std::io::ErrorKind::InvalidData,
                                        ERR_TASKREG_TASK_NOT_ADDED,
                                    ));
                                }
                            } else {
                                log(
                                    LogType::Trace,
                                    LOG_EMITTER_CONFIGURATION,
                                    LOG_ACTION_RECONFIGURE,
                                    None,
                                    LOG_WHEN_PROC,
                                    LOG_STATUS_MSG,
                                    &format!("not reconfiguring task {task_name}: no change detected"),
                                )
                            }
                            if to_remove.contains(&task_name) {
                                to_remove.swap_remove(to_remove.iter().position(|x| task_name == *x).unwrap());
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

    // remove conditions that have not been found in new configuration
    for name in to_remove {
        log(
            LogType::Trace,
            LOG_EMITTER_CONFIGURATION,
            LOG_ACTION_RECONFIGURE,
            None,
            LOG_WHEN_PROC,
            LOG_STATUS_MSG,
            &format!("removing task {name} from registry"),
        );
        task_registry.dynamic_remove_task(&name)?;
    }

    Ok(())
}



// configure conditions according to the provided configuration map
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


// reconfigure conditions according to the provided configuration map
fn reconfigure_conditions(
    cfgmap: &CfgMap,
    cond_registry: &'static ConditionRegistry,
    task_registry: &'static TaskRegistry,
    bucket: &'static ExecutionBucket,
    tick_secs: u64,
) -> std::io::Result<()> {

    let mut to_remove: Vec<String> = Vec::new();
    let e = cond_registry.condition_names();
    if e.is_some() {
        to_remove = e.unwrap().clone();
    }

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
                            let cond_name = condition.get_name();
                            if !cond_registry.has_condition(&cond_name) || !cond_registry.has_condition_eq(&condition) {
                                if !cond_registry.dynamic_add_or_replace_condition(Box::new(condition))? {
                                    return Err(std::io::Error::new(
                                        std::io::ErrorKind::InvalidData,
                                        ERR_CONDREG_COND_NOT_ADDED,
                                    ));
                                }
                            } else {
                                log(
                                    LogType::Trace,
                                    LOG_EMITTER_CONFIGURATION,
                                    LOG_ACTION_RECONFIGURE,
                                    None,
                                    LOG_WHEN_PROC,
                                    LOG_STATUS_MSG,
                                    &format!("not reconfiguring condition {cond_name}: no change detected"),
                                )
                            }
                            if to_remove.contains(&cond_name) {
                                to_remove.swap_remove(to_remove.iter().position(|x| cond_name == *x).unwrap());
                            }
                        }
                        "idle" => {
                            let condition = condition::idle_cond::IdleCondition::load_cfgmap(
                                entry.as_map().unwrap(), task_registry)?;
                            let cond_name = condition.get_name();
                            if !cond_registry.has_condition(&cond_name) || !cond_registry.has_condition_eq(&condition) {
                                if !cond_registry.dynamic_add_or_replace_condition(Box::new(condition))? {
                                    return Err(std::io::Error::new(
                                        std::io::ErrorKind::InvalidData,
                                        ERR_CONDREG_COND_NOT_ADDED,
                                    ));
                                }
                            } else {
                                log(
                                    LogType::Trace,
                                    LOG_EMITTER_CONFIGURATION,
                                    LOG_ACTION_RECONFIGURE,
                                    None,
                                    LOG_WHEN_PROC,
                                    LOG_STATUS_MSG,
                                    &format!("not reconfiguring condition {cond_name}: no change detected"),
                                )
                            }
                            if to_remove.contains(&cond_name) {
                                to_remove.swap_remove(to_remove.iter().position(|x| cond_name == *x).unwrap());
                            }
                        }
                        "time" => {
                            // this is peculiar because it requires extra initialization after loading from map
                            let mut condition = condition::time_cond::TimeCondition::load_cfgmap(
                                entry.as_map().unwrap(), task_registry)?;
                            let _ = condition.set_tick_duration(tick_secs)?;
                            let cond_name = condition.get_name();
                            if !cond_registry.has_condition(&cond_name) || !cond_registry.has_condition_eq(&condition) {
                                if !cond_registry.dynamic_add_or_replace_condition(Box::new(condition))? {
                                    return Err(std::io::Error::new(
                                        std::io::ErrorKind::InvalidData,
                                        ERR_CONDREG_COND_NOT_ADDED,
                                    ));
                                }
                            } else {
                                log(
                                    LogType::Trace,
                                    LOG_EMITTER_CONFIGURATION,
                                    LOG_ACTION_RECONFIGURE,
                                    None,
                                    LOG_WHEN_PROC,
                                    LOG_STATUS_MSG,
                                    &format!("not reconfiguring condition {cond_name}: no change detected"),
                                )
                            }
                            if to_remove.contains(&cond_name) {
                                to_remove.swap_remove(to_remove.iter().position(|x| cond_name == *x).unwrap());
                            }
                        }
                        "command" => {
                            let condition = condition::command_cond::CommandCondition::load_cfgmap(
                                entry.as_map().unwrap(), task_registry)?;
                                let cond_name = condition.get_name();
                                if !cond_registry.has_condition(&cond_name) || cond_registry.has_condition_eq(&condition) {
                                    if !cond_registry.dynamic_add_or_replace_condition(Box::new(condition))? {
                                        return Err(std::io::Error::new(
                                            std::io::ErrorKind::InvalidData,
                                            ERR_CONDREG_COND_NOT_ADDED,
                                        ));
                                    }
                                } else {
                                    log(
                                        LogType::Trace,
                                        LOG_EMITTER_CONFIGURATION,
                                        LOG_ACTION_RECONFIGURE,
                                        None,
                                        LOG_WHEN_PROC,
                                        LOG_STATUS_MSG,
                                        &format!("not reconfiguring condition {cond_name}: no change detected"),
                                    )
                                }
                                if to_remove.contains(&cond_name) {
                                    to_remove.swap_remove(to_remove.iter().position(|x| cond_name == *x).unwrap());
                                }
                                }
                        "lua" => {
                            let condition = condition::lua_cond::LuaCondition::load_cfgmap(
                                entry.as_map().unwrap(), task_registry)?;
                            let cond_name = condition.get_name();
                            if !cond_registry.has_condition(&cond_name) || cond_registry.has_condition_eq(&condition) {
                                if !cond_registry.dynamic_add_or_replace_condition(Box::new(condition))? {
                                    return Err(std::io::Error::new(
                                        std::io::ErrorKind::InvalidData,
                                        ERR_CONDREG_COND_NOT_ADDED,
                                    ));
                                }
                            } else {
                                log(
                                    LogType::Trace,
                                    LOG_EMITTER_CONFIGURATION,
                                    LOG_ACTION_RECONFIGURE,
                                    None,
                                    LOG_WHEN_PROC,
                                    LOG_STATUS_MSG,
                                    &format!("not reconfiguring condition {cond_name}: no change detected"),
                                )
                            }
                            if to_remove.contains(&cond_name) {
                                to_remove.swap_remove(to_remove.iter().position(|x| cond_name == *x).unwrap());
                            }
                        }
                        "dbus" => {
                            let condition = condition::dbus_cond::DbusMethodCondition::load_cfgmap(
                                entry.as_map().unwrap(), task_registry)?;
                            let cond_name = condition.get_name();
                            if !cond_registry.has_condition(&cond_name) || cond_registry.has_condition_eq(&condition) {
                                if !cond_registry.dynamic_add_or_replace_condition(Box::new(condition))? {
                                    return Err(std::io::Error::new(
                                        std::io::ErrorKind::InvalidData,
                                        ERR_CONDREG_COND_NOT_ADDED,
                                    ));
                                }
                            } else {
                                log(
                                    LogType::Trace,
                                    LOG_EMITTER_CONFIGURATION,
                                    LOG_ACTION_RECONFIGURE,
                                    None,
                                    LOG_WHEN_PROC,
                                    LOG_STATUS_MSG,
                                    &format!("not reconfiguring condition {cond_name}: no change detected"),
                                )
                            }
                            if to_remove.contains(&cond_name) {
                                to_remove.swap_remove(to_remove.iter().position(|x| cond_name == *x).unwrap());
                            }
                        }
                        "bucket" | "event" => {
                            // this is peculiar because it requires extra initialization after loading from map
                            let mut condition = condition::bucket_cond::BucketCondition::load_cfgmap(
                                entry.as_map().unwrap(), task_registry)?;
                            let _ = condition.set_execution_bucket(bucket)?;
                            let cond_name = condition.get_name();
                            if !cond_registry.has_condition(&cond_name) || cond_registry.has_condition_eq(&condition) {
                                if !cond_registry.dynamic_add_or_replace_condition(Box::new(condition))? {
                                    return Err(std::io::Error::new(
                                        std::io::ErrorKind::InvalidData,
                                        ERR_CONDREG_COND_NOT_ADDED,
                                    ));
                                }
                            } else {
                                log(
                                    LogType::Trace,
                                    LOG_EMITTER_CONFIGURATION,
                                    LOG_ACTION_RECONFIGURE,
                                    None,
                                    LOG_WHEN_PROC,
                                    LOG_STATUS_MSG,
                                    &format!("not reconfiguring condition {cond_name}: no change detected"),
                                )
                            }
                            if to_remove.contains(&cond_name) {
                                to_remove.swap_remove(to_remove.iter().position(|x| cond_name == *x).unwrap());
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

    // remove conditions that have not been found in new configuration
    for name in to_remove {
        log(
            LogType::Trace,
            LOG_EMITTER_CONFIGURATION,
            LOG_ACTION_RECONFIGURE,
            None,
            LOG_WHEN_PROC,
            LOG_STATUS_MSG,
            &format!("removing condition {name} from registry"),
        );
        cond_registry.dynamic_remove_condition(&name)?;
    }

    Ok(())
}



// configure events according to the provided configuration map
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


// reconfigure events according to the provided configuration map
fn reconfigure_events(
    cfgmap: &CfgMap,
    event_registry: &'static EventRegistry,
    cond_registry: &'static ConditionRegistry,
    bucket: &'static ExecutionBucket,
) -> std::io::Result<()> {

    let mut to_remove: Vec<String> = Vec::new();
    let e = event_registry.event_names();
    if e.is_some() {
        to_remove = e.unwrap().clone();
    }

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
                            if !event_registry.has_event(&event_name) || !event_registry.has_event_eq(&event) {
                                if event_registry.has_event(&event_name) {
                                    event_registry.unlisten_for(&event_name)?;
                                    event_registry.remove_event(&event_name)?;
                                }
                                if !event_registry.add_event(Box::new(event))? {
                                    return Err(std::io::Error::new(
                                        std::io::ErrorKind::InvalidData,
                                        ERR_EVENTREG_EVENT_NOT_ADDED,
                                    ));
                                } else if event_registry.listen_for(&event_name).is_ok() {
                                    log(
                                        LogType::Trace,
                                        LOG_EMITTER_CONFIGURATION,
                                        LOG_ACTION_MAIN_LISTENER,
                                        None,
                                        LOG_WHEN_PROC,
                                        LOG_STATUS_MSG,
                                        &format!("service installed for event {event_name} (dedicated thread)"),
                                    );
                                } else {
                                    return Err(std::io::Error::new(
                                        std::io::ErrorKind::InvalidData,
                                        ERR_EVENTREG_EVENT_NOT_ADDED,
                                    ));
                                }
                            } else {
                                log(
                                    LogType::Trace,
                                    LOG_EMITTER_CONFIGURATION,
                                    LOG_ACTION_RECONFIGURE,
                                    None,
                                    LOG_WHEN_PROC,
                                    LOG_STATUS_MSG,
                                    &format!("not reconfiguring event {event_name}: no change detected"),
                                );
                            }
                            if to_remove.contains(&event_name) {
                                to_remove.swap_remove(to_remove.iter().position(|x| event_name == *x).unwrap());
                            }
                        }
                        "dbus" => {
                            let event = event::dbus_event::DbusMessageEvent::load_cfgmap(
                                entry.as_map().unwrap(), cond_registry, bucket)?;
                            let event_name = event.get_name();
                            if !event_registry.has_event(&event_name) || !event_registry.has_event_eq(&event) {
                                if event_registry.has_event(&event_name) {
                                    event_registry.unlisten_for(&event_name)?;
                                    event_registry.remove_event(&event_name)?;
                                }
                                if !event_registry.add_event(Box::new(event))? {
                                    return Err(std::io::Error::new(
                                        std::io::ErrorKind::InvalidData,
                                        ERR_EVENTREG_EVENT_NOT_ADDED,
                                    ));
                                } else if event_registry.listen_for(&event_name).is_ok() {
                                    log(
                                        LogType::Trace,
                                        LOG_EMITTER_CONFIGURATION,
                                        LOG_ACTION_MAIN_LISTENER,
                                        None,
                                        LOG_WHEN_PROC,
                                        LOG_STATUS_MSG,
                                        &format!("service installed for event {event_name} (dedicated thread)"),
                                    );
                                } else {
                                    return Err(std::io::Error::new(
                                        std::io::ErrorKind::InvalidData,
                                        ERR_EVENTREG_EVENT_NOT_ADDED,
                                    ));
                                }
                            } else {
                                log(
                                    LogType::Trace,
                                    LOG_EMITTER_CONFIGURATION,
                                    LOG_ACTION_RECONFIGURE,
                                    None,
                                    LOG_WHEN_PROC,
                                    LOG_STATUS_MSG,
                                    &format!("not reconfiguring event {event_name}: no change detected"),
                                );
                            }
                            if to_remove.contains(&event_name) {
                                to_remove.swap_remove(to_remove.iter().position(|x| event_name == *x).unwrap());
                            }
                        }
                        "cli" => {
                            let event = event::manual_event::ManualCommandEvent::load_cfgmap(
                                entry.as_map().unwrap(), cond_registry, bucket)?;
                            let event_name = event.get_name();
                            if !event_registry.has_event(&event_name) || !event_registry.has_event_eq(&event) {
                                if event_registry.has_event(&event_name) {
                                    event_registry.unlisten_for(&event_name)?;
                                    event_registry.remove_event(&event_name)?;
                                }
                                if !event_registry.add_event(Box::new(event))? {
                                    return Err(std::io::Error::new(
                                        std::io::ErrorKind::InvalidData,
                                        ERR_EVENTREG_EVENT_NOT_ADDED,
                                    ));
                                } else if event_registry.listen_for(&event_name).is_ok() {
                                    log(
                                        LogType::Trace,
                                        LOG_EMITTER_CONFIGURATION,
                                        LOG_ACTION_MAIN_LISTENER,
                                        None,
                                        LOG_WHEN_PROC,
                                        LOG_STATUS_MSG,
                                        &format!("service installed for event {event_name}"),
                                    );
                                } else {
                                    return Err(std::io::Error::new(
                                        std::io::ErrorKind::InvalidData,
                                        ERR_EVENTREG_EVENT_NOT_ADDED,
                                    ));
                                }
                            } else {
                                log(
                                    LogType::Trace,
                                    LOG_EMITTER_CONFIGURATION,
                                    LOG_ACTION_RECONFIGURE,
                                    None,
                                    LOG_WHEN_PROC,
                                    LOG_STATUS_MSG,
                                    &format!("not reconfiguring event {event_name}: no change detected"),
                                );
                            }
                            if to_remove.contains(&event_name) {
                                to_remove.swap_remove(to_remove.iter().position(|x| event_name == *x).unwrap());
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

    // remove events that have not been found in new configuration
    for name in to_remove {
        log(
            LogType::Trace,
            LOG_EMITTER_CONFIGURATION,
            LOG_ACTION_RECONFIGURE,
            None,
            LOG_WHEN_PROC,
            LOG_STATUS_MSG,
            &format!("removing event {name} from registry"),
        );
        event_registry.unlisten_for(&name)?;
        event_registry.remove_event(&name)?;
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

/// Reconfigure all items given a configuration map
pub fn reconfigure_items(
    cfgmap: &CfgMap,
    task_registry: &'static TaskRegistry,
    cond_registry: &'static ConditionRegistry,
    event_registry: &'static EventRegistry,
    bucket: &'static ExecutionBucket,
    tick_secs: u64,
) -> std::io::Result<()> {
    reconfigure_tasks(cfgmap, task_registry)?;
    reconfigure_conditions(cfgmap, cond_registry, task_registry, bucket, tick_secs)?;
    reconfigure_events(cfgmap, event_registry, cond_registry, bucket)?;
    Ok(())
}


// end.
