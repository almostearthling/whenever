//! Define event based on DBus subscriptions
//!
//! The user subscribes to a DBus event, specifying what to listen to and what
//! to expect from that channel. The event listens on a new thread and pushes
//! the related condition in the execution bucket when all constraints are met.


use cfgmap::{CfgMap, CfgValue};
use regex::Regex;

use async_std::task;
use zbus::{self, AsyncDrop};
use zbus::export::futures_util::TryStreamExt;
use zbus::zvariant;

use std::str::FromStr;
use serde_json::value::Value;


use super::base::Event;
use crate::condition::registry::ConditionRegistry;
use crate::condition::bucket_cond::ExecutionBucket;
use crate::common::logging::{log, LogType};
use crate::constants::*;


// see the DBus specification
const DBUS_MAX_NUMBER_OF_ARGUMENTS: i64 = 63;


/// an enum to store the operators for checking signal parameters
#[derive(PartialEq)]
enum ParamCheckOperator {
    Equal,              // "eq"
    NotEqual,           // "neq"
    Greater,            // "gt"
    GreaterEqual,       // "ge"
    Less,               // "lt"
    LessEqual,          // "le"
    Match,              // "match"
    Contains,           // "contains"
    NotContains,        // "ncontains"
}

/// an enum containing the value that the parameter should be checked against
enum ParameterCheckValue {
    Boolean(bool),
    Integer(i64),
    Float(f64),
    String(String),
    Regex(Regex),
}

/// an enum containing the possible types of indexes for parameters
enum ParameterIndex {
    Integer(u64),
    String(String),
}

/// a struct containing a single test to be performed against a signal payload
///
/// short explaination, so that I remember how to use it:
/// - `Index`: contains a list of indexes which specify, also for nested
///            structures. This means that for an array of mappings it might
///            be of the form `{ 1, 3, "somepos" }` where the first `1` is the
///            argument index, the `3` is the array index, and `"somepos"` is
///            the mapping index.
/// - `Operator`: the operator to check the payload against
/// - `Value`: the value to compare the parameter entry to
struct ParameterCheckTest {
    index: Vec<ParameterIndex>,
    operator: ParamCheckOperator,
    value: ParameterCheckValue,
}


/// a trait that defines containable types: implementations are provided for
/// all types found in the `ParameterCheckValue` enum defined above
trait Containable {
    fn is_contained_in(self, v: &zvariant::Value) -> bool;
}

// implementations: dictionary value lookup will be provided as soon as there
// will be a way, in _zbus_, to at least retrieve the dictionary keys (if not
// directly the mapped values) in order to compare the contents with the value
impl Containable for bool {
    fn is_contained_in(self, v: &zvariant::Value) -> bool {
        match v {
            zvariant::Value::Array(a) => {
                a.contains(&zvariant::Value::from(self))
            }
            _ => false
        }
    }
}

impl Containable for i64 {
    fn is_contained_in(self, v: &zvariant::Value) -> bool {
        match v {
            zvariant::Value::Array(a) => {
                // to handle this we transform the array into a new array of
                // i64 that is created to test for inclusion, and large u64
                // numbers are be automatically discarded and set to `None`
                // which is never matched
                let testv: Vec<Option<i64>>;
                match a.element_signature().as_str() {
                    "y" => {    // BYTE
                        testv = a.iter().map(|x| {
                            if let zvariant::Value::U8(z) = x {
                                Some(i64::from(*z))
                            } else {
                                None
                            }
                        }).collect();
                    }
                    "n" => {    // INT16
                        testv = a.iter().map(|x| {
                            if let zvariant::Value::I16(z) = x {
                                Some(i64::from(*z))
                            } else {
                                None
                            }
                        }).collect();
                    }
                    "q" => {    // UINT16
                        testv = a.iter().map(|x| {
                            if let zvariant::Value::I16(z) = x {
                                Some(i64::from(*z))
                            } else {
                                None
                            }
                        }).collect();
                    }
                    "i" => {    // INT32
                        testv = a.iter().map(|x| {
                            if let zvariant::Value::I32(z) = x {
                                Some(i64::from(*z))
                            } else {
                                None
                            }
                        }).collect();
                    }
                    "u" => {    // UINT32
                        testv = a.iter().map(|x| {
                            if let zvariant::Value::U32(z) = x {
                                Some(i64::from(*z))
                            } else {
                                None
                            }
                        }).collect();
                    }
                    "x" => {    // INT64
                        testv = a.iter().map(|x| {
                            if let zvariant::Value::I64(z) = x {
                                Some(i64::from(*z))
                            } else {
                                None
                            }
                        }).collect();
                    }
                    "t" => {    // UINT64
                        // this is the tricky one, but since we know that big
                        // unsigned integer surely do not match the provided
                        // value, we just convert them to `None` here, which
                        // will never match
                        testv = a.iter().map(|x| {
                            if let zvariant::Value::U64(z) = x {
                                if *z <= i64::MAX as u64 {
                                    Some(*z as i64)
                                } else {
                                    None
                                }
                            } else {
                                None
                            }
                        }).collect();
                    }
                    _ => { return false; }
                }
                testv.contains(&Some(self))
            }
            _ => false
        }
    }
}

impl Containable for f64 {
    fn is_contained_in(self, v: &zvariant::Value) -> bool {
        match v {
            zvariant::Value::Array(a) => {
                a.contains(&zvariant::Value::from(self))
            }
            _ => false
        }
    }
}

// String is a particular case, because it has to look for presence in arrays
// (both of `Str` and `ObjectPath`) or, alternatively, to match a substring
// of the returned `Str` or `ObjectPath`
impl Containable for String {
    fn is_contained_in(self, v: &zvariant::Value) -> bool {
        match v {
            zvariant::Value::Str(s) => {
                s.as_str().contains(self.as_str())
            }
            zvariant::Value::ObjectPath(s) => {
                s.as_str().contains(self.as_str())
            }
            zvariant::Value::Array(a) => {
                match a.element_signature().as_str() {
                    "s" => {
                        a.contains(&zvariant::Value::from(self))
                    }
                    "o" => {
                        let o = zvariant::ObjectPath::try_from(self);
                        if let Ok(o) = o {
                            a.contains(&zvariant::Value::from(o))
                        } else {
                            false
                        }
                    }
                    _ => false
                }
            }
            _ => false
        }
    }
}

// the following is totally arbitrary and should not be actually used: it is
// provided here only in order to complete the "required" implementations
impl Containable for Regex {
    fn is_contained_in(self, v: &zvariant::Value) -> bool {
        match v {
            zvariant::Value::Array(a) => {
                for elem in a.to_vec() {
                    if let zvariant::Value::Str(s) = elem {
                        if self.is_match(s.as_str()) {
                            return true;
                        }
                    }
                }
                false
            }
            _ => false
        }
    }
}



/// DBus Based Event
///
/// Implements an event based upon DBus suscription to certain events, using
/// the [zbus](https://docs.rs/zbus/latest/zbus/) cross-platform pure Rust
/// DBus library. Configurations are provided for the different platforms.
///
/// **Note**: the `match_rule` holds a string implementing the *match rules*:
///           see [match rules](https://dbus.freedesktop.org/doc/dbus-specification.html#message-bus-routing-match-rules)
///           in the DBus specification for the exact (formal) syntax.
pub struct DbusMessageEvent {
    // common members
    // parameters
    event_id: i64,
    event_name: String,
    condition_name: Option<String>,

    // internal values
    condition_registry: Option<&'static ConditionRegistry>,
    condition_bucket: Option<&'static ExecutionBucket>,

    // specific members
    // parameters
    bus: Option<String>,
    match_rule: Option<String>,
    param_checks: Option<Vec<ParameterCheckTest>>,
    param_checks_all: bool,

    // internal values
    // (none here)
}


#[allow(dead_code)]
impl DbusMessageEvent {

    /// Create a new `DbusMessageEvent` with the provided name
    pub fn new(name: &str) -> Self {
        log(
            LogType::Debug,
            LOG_EMITTER_EVENT_DBUS,
            LOG_ACTION_NEW,
            Some((name, 0)),
            LOG_WHEN_INIT,
            LOG_STATUS_MSG,
            &format!("EVENT {name}: creating a new DBus signal based event"),
        );
        DbusMessageEvent {
            // reset ID
            event_id: 0,

            // parameters
            event_name: String::from(name),
            condition_name: None,

            // internal values
            condition_registry: None,
            condition_bucket: None,

            // specific members initialization
            // parameters
            bus: None,
            match_rule: None,
            param_checks: None,
            param_checks_all: false,

            // internal values
        }
    }


    /// Set the bus name to the provided value (checks for validity)
    pub fn set_bus(&mut self, name: &str) -> bool {
        if RE_DBUS_BUS_NAME.is_match(name) {
            self.bus = Some(String::from(name));
            return true;
        }
        false
    }

    /// Return an owned copy of the bus name
    pub fn bus(&self) -> Option<String> { self.bus.clone() }

    /// Set the match rule to the provided value (will check upon installation)
    pub fn set_match_rule(&mut self, rule: &str) -> bool {
        self.match_rule = Some(String::from(rule));
        true
    }

    /// Return an owned copy of the signal name
    pub fn match_rule(&self) -> Option<String> { self.match_rule.clone() }


    /// Load a `DbusMessageEvent` from a [`CfgMap`](https://docs.rs/cfgmap/latest/)
    ///
    /// The `DbusMessageEvent` is initialized according to the values
    /// provided in the `CfgMap` argument. If the `CfgMap` format does not
    /// comply with the requirements of a `DbusMessageEvent` an error is
    /// raised.
    ///
    /// Note that the values for the `parameter_check` entry are provided as
    /// JSON strings, because TOML is intentionally limited to accepting only
    /// lists of elements of the same type, and in our case we need to mix
    /// types both as arguments to a call and as index sequences.
    ///
    /// The TOML configuration file format is the following
    ///
    /// ```toml
    /// # definition (mandatory)
    /// [[event]]
    /// name = "DbusMessageEventName"
    /// type = "dbus"                       # mandatory value
    /// bus = ":session"                    # either ":session" or ":system"
    /// condition = "AssignedConditionName"
    /// rule = """\
    ///     type='signal',\
    ///     sender='org.freedesktop.DBus',\
    ///     interface='org.freedesktop.DBus',\
    ///     member='NameOwnerChanged',\
    ///     arg0='org.freedesktop.zbus.MatchRuleStreamTest42'\
    /// """
    ///
    /// # optional parameters (if omitted, defaults are used)
    /// parameter_check_all = false
    /// parameter_check = """[
    ///          { "index": 0, "operator": "eq", "value": false },
    ///          { "index": [1, 5], "operator": "neq", "value": "forbidden" },
    ///          {
    ///              "index": [2, "mapidx", 5],
    ///              "operator": "match",
    ///              "value": "^[A-Z][a-zA-Z0-9_]*$"
    ///          }
    ///     ]"""
    /// ```
    ///
    /// The `rule` parameter must comply with the [match rule specification](
    /// https://dbus.freedesktop.org/doc/dbus-specification.html#message-bus-routing-match-rules),
    /// otherwise the service will not be able to listen to messages/signals.
    ///
    /// Parameter checks should be _valid_: this means that indexes, be they
    /// integers or strings, must be used where they are accepted (that is, an
    /// integer is only valid in arrays and a string in maps), and operators
    /// as well: only numbers support order operators, strings only support
    /// equality or inequality, booleans only support equality, regular
    /// expression only support matching. Unsupported cases will render all
    /// tests _false_, and a warning will be issued. Also, the first element
    /// of a parameter traversal list _must_ be an integer, as parameters are
    /// provided as a tuple-like object: using a string will not result in an
    /// error, but will cause all parameter checks to fail.
    ///
    /// Any incorrect value will cause an error. The value of the `type` entry
    /// *must* be set to `"dbus"` mandatorily for this type of `Event`.
    pub fn load_cfgmap(
        cfgmap: &CfgMap,
        cond_registry: &'static ConditionRegistry,
        bucket: &'static ExecutionBucket,
    ) -> std::io::Result<DbusMessageEvent> {

        fn _invalid_cfg(key: &str, value: &str, message: &str) -> std::io::Result<DbusMessageEvent> {
            Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("{ERR_INVALID_EVENT_CONFIG}: ({key}={value}) {message}"),
            ))
        }

        fn _check_dbus_param_index(index: &CfgValue) -> Option<ParameterIndex> {
            if index.is_int() {
                let i = *index.as_int().unwrap();
                // as per specification, DBus supports at most 64 parameters
                if !(0..=DBUS_MAX_NUMBER_OF_ARGUMENTS).contains(&i) {
                    return None;
                } else {
                    return Some(ParameterIndex::Integer(i as u64));
                }
            } else if index.is_str() {
                let s = String::from(index.as_str().unwrap());
                return Some(ParameterIndex::String(s));
            }
            None
        }

        let check = [
            "type",
            "name",
            "tags",
            "condition",
            "bus",
            "rule",
            "parameter_check",
            "parameter_check_all",
        ];
        for key in cfgmap.keys() {
            if !check.contains(&key.as_str()) {
                return _invalid_cfg(key, STR_UNKNOWN_VALUE,
                    &format!("{ERR_INVALID_CFG_ENTRY} ({key})"));
            }
        }

        // check type
        let cur_key = "type";
        let cond_type;
        if let Some(item) = cfgmap.get(cur_key) {
            if !item.is_str() {
                return _invalid_cfg(
                    cur_key,
                    STR_UNKNOWN_VALUE,
                    ERR_INVALID_EVENT_TYPE);
            }
            cond_type = item.as_str().unwrap().to_owned();
            if cond_type != "dbus" {
                return _invalid_cfg(cur_key,
                    &cond_type,
                    ERR_INVALID_EVENT_TYPE);
            }
        } else {
            return _invalid_cfg(
                cur_key,
                STR_UNKNOWN_VALUE,
                ERR_MISSING_PARAMETER);
        }

        // common mandatory parameter retrieval
        let cur_key = "name";
        let name;
        if let Some(item) = cfgmap.get(cur_key) {
            if !item.is_str() {
                return _invalid_cfg(
                    cur_key,
                    STR_UNKNOWN_VALUE,
                    ERR_INVALID_EVENT_NAME);
            }
            name = item.as_str().unwrap().to_owned();
            if !RE_EVENT_NAME.is_match(&name) {
                return _invalid_cfg(cur_key,
                    &name,
                    ERR_INVALID_EVENT_NAME);
            }
        } else {
            return _invalid_cfg(
                cur_key,
                STR_UNKNOWN_VALUE,
                ERR_MISSING_PARAMETER);
        }

        // specific mandatory parameter initialization
        let cur_key = "bus";
        let bus;
        if let Some(item) = cfgmap.get(cur_key) {
            if !item.is_str() {
                return _invalid_cfg(
                    cur_key,
                    STR_UNKNOWN_VALUE,
                    ERR_INVALID_EVENT_NAME);
            }
            bus = item.as_str().unwrap().to_owned();
            if !RE_DBUS_MSGBUS_NAME.is_match(&bus) {
                return _invalid_cfg(cur_key,
                    &bus,
                    ERR_INVALID_VALUE_FOR_ENTRY);
            }
        } else {
            return _invalid_cfg(
                cur_key,
                STR_UNKNOWN_VALUE,
                ERR_MISSING_PARAMETER);
        }

        let cur_key = "rule";
        let rule;
        if let Some(item) = cfgmap.get(cur_key) {
            if !item.is_str() {
                return _invalid_cfg(
                    cur_key,
                    STR_UNKNOWN_VALUE,
                    ERR_INVALID_VALUE_FOR_ENTRY);
            }
            rule = item.as_str().unwrap().to_owned();
        } else {
            return _invalid_cfg(
                cur_key,
                STR_UNKNOWN_VALUE,
                ERR_MISSING_PARAMETER);
        }

        // initialize the structure
        // NOTE: the value of "event" for the condition type, which is
        //       completely functionally equivalent to "bucket", can only
        //       be set from the configuration file; programmatically built
        //       conditions of this type will only report "bucket" as their
        //       type, and "event" is only left for configuration readability
        let mut new_event = DbusMessageEvent::new(
            &name,
        );
        new_event.condition_registry = Some(cond_registry);
        new_event.condition_bucket = Some(bucket);
        new_event.bus = Some(bus);
        new_event.match_rule = Some(rule);

        // common optional parameter initialization
        let cur_key = "tags";
        if let Some(item) = cfgmap.get(cur_key) {
            if !item.is_list() && !item.is_map() {
                return _invalid_cfg(
                    cur_key,
                    STR_UNKNOWN_VALUE,
                    ERR_INVALID_PARAMETER);
            }
        }

        let cur_key = "condition";
        let condition;
        if let Some(item) = cfgmap.get(cur_key) {
            if !item.is_str() {
                return _invalid_cfg(
                    cur_key,
                    STR_UNKNOWN_VALUE,
                    ERR_INVALID_COND_NAME);
            }
            condition = item.as_str().unwrap().to_owned();
            if !RE_COND_NAME.is_match(&condition) {
                return _invalid_cfg(cur_key,
                    &condition,
                    ERR_INVALID_COND_NAME);
            }
        } else {
            return _invalid_cfg(
                cur_key,
                STR_UNKNOWN_VALUE,
                ERR_MISSING_PARAMETER);
        }
        if !new_event.condition_registry.unwrap().has_condition(&condition) {
            return _invalid_cfg(
                cur_key,
                &condition,
                ERR_INVALID_EVENT_CONDITION);
        }
        new_event.assign_condition(&condition)?;

        // specific optional parameter initialization

        // this is tricky: we build a list of elements constituted by:
        // - an index list (integers and strings, mixed) which will address
        //   every nested structure,
        // - an operator,
        // - a value to check against using the operator;
        // of course the value types found in TOML are less tructured than the
        // ones supported by DBus, and subsequent tests will take this into
        // account and compare only values compatible with each other, and
        // compatible with the operator used
        let check = [
            "index",
            "operator",
            "value",
        ];

        let cur_key = "parameter_check";
        if let Some(item) = cfgmap.get(cur_key) {
            let mut param_checks: Vec<ParameterCheckTest> = Vec::new();
            // here we expect a JSON string, reason explained above
            if !item.is_str() {
                return _invalid_cfg(
                    cur_key,
                    STR_UNKNOWN_VALUE,
                    ERR_INVALID_VALUE_FOR_ENTRY);
            }
            // since CfgMap only accepts maps as input, and we expect a list
            // instead, we build a map with a single element labeled '0':
            let json = Value::from_str(
                &format!("{{\"0\": {}}}", item.as_str().unwrap())
            );
            if json.is_err() {
                return _invalid_cfg(
                    cur_key,
                    STR_UNKNOWN_VALUE,
                    ERR_INVALID_VALUE_FOR_ENTRY);
            }
            // and then we extract the '0' element and check it to be a list
            let item = CfgMap::from_json(json.unwrap());
            let item = item.get("0").unwrap();
            if !item.is_list() {
                return _invalid_cfg(
                    cur_key,
                    STR_UNKNOWN_VALUE,
                    ERR_INVALID_VALUE_FOR_ENTRY);
            }
            let item = item.as_list().unwrap();
            for spec in item.iter() {
                if !spec.is_map() {
                    return _invalid_cfg(
                        cur_key,
                        STR_UNKNOWN_VALUE,
                        ERR_INVALID_VALUE_FOR_ENTRY);
                }
                let spec = spec.as_map().unwrap();
                for key in spec.keys() {
                    if !check.contains(&key.as_str()) {
                        return _invalid_cfg(
                            &format!("{cur_key}:{key}"),
                            STR_UNKNOWN_VALUE,
                            &format!("{ERR_INVALID_CFG_ENTRY} ({key})"));
                    }
                }
                let mut index_list: Vec<ParameterIndex> = Vec::new();
                if let Some(index) = spec.get("index") {
                    if index.is_int() {
                        if let Some(px) = _check_dbus_param_index(index) {
                            index_list.push(px);
                        } else {
                            return _invalid_cfg(
                                &format!("{cur_key}:index"),
                                &format!("{index:?}"),
                                ERR_INVALID_VALUE_FOR_ENTRY);
                        }
                    } else if index.is_list() {
                        for sub_index in index.as_list().unwrap() {
                            if let Some(px) = _check_dbus_param_index(sub_index) {
                                index_list.push(px);
                            } else {
                                return _invalid_cfg(
                                    &format!("{cur_key}:index"),
                                    &format!("{sub_index:?}"),
                                    ERR_INVALID_VALUE_FOR_ENTRY);
                            }
                        }
                    } else {
                        return _invalid_cfg(
                            &format!("{cur_key}:index"),
                            &format!("{index:?}"),
                            ERR_INVALID_VALUE_FOR_ENTRY);
                    }
                } else {
                    return _invalid_cfg(
                        &format!("{cur_key}:index"),
                        STR_UNKNOWN_VALUE,
                        ERR_MISSING_PARAMETER);
                }

                let operator;
                if let Some(oper) = spec.get("operator") {
                    if oper.is_str() {
                        operator = match oper.as_str().unwrap().as_str() {
                            "eq" => ParamCheckOperator::Equal,
                            "neq" => ParamCheckOperator::NotEqual,
                            "gt" => ParamCheckOperator::Greater,
                            "ge" => ParamCheckOperator::GreaterEqual,
                            "lt" => ParamCheckOperator::Less,
                            "le" => ParamCheckOperator::LessEqual,
                            "match" => ParamCheckOperator::Match,
                            "contains" => ParamCheckOperator::Contains,
                            "ncontains" => ParamCheckOperator::NotContains,
                            _ => {
                                return _invalid_cfg(
                                    &format!("{cur_key}:operator"),
                                    &format!("{oper:?}"),
                                    ERR_INVALID_VALUE_FOR_ENTRY);
                            }
                        };
                    } else {
                        return _invalid_cfg(
                            &format!("{cur_key}:operator"),
                            STR_UNKNOWN_VALUE,
                            ERR_INVALID_VALUE_FOR_ENTRY);
                    }
                } else {
                    return _invalid_cfg(
                        &format!("{cur_key}:operator"),
                        STR_UNKNOWN_VALUE,
                        ERR_MISSING_PARAMETER);
                }

                let value;
                if let Some(v) = spec.get("value") {
                    if v.is_bool() {
                        value = ParameterCheckValue::Boolean(*v.as_bool().unwrap());
                    } else if v.is_int() {
                        value = ParameterCheckValue::Integer(*v.as_int().unwrap());
                    } else if v.is_float() {
                        value = ParameterCheckValue::Float(*v.as_float().unwrap());
                    } else if v.is_str() {
                        let s = v.as_str().unwrap();
                        if operator == ParamCheckOperator::Match {
                            let re = Regex::from_str(s);
                            if let Ok(re) = re {
                                value = ParameterCheckValue::Regex(re);
                            } else {
                                return _invalid_cfg(
                                    &format!("{cur_key}:value"),
                                    &format!("{v:?}"),
                                    ERR_INVALID_VALUE_FOR_ENTRY);
                            }
                        } else {
                            value = ParameterCheckValue::String(s.to_string());
                        }
                    } else {
                        return _invalid_cfg(
                            &format!("{cur_key}:value"),
                            STR_UNKNOWN_VALUE,
                            ERR_INVALID_VALUE_FOR_ENTRY);
                    }
                } else {
                    return _invalid_cfg(
                        &format!("{cur_key}:value"),
                        STR_UNKNOWN_VALUE,
                        ERR_MISSING_PARAMETER);
                }
                // now that we have the full triple, we can add it to criteria
                param_checks.push(ParameterCheckTest { index: index_list, operator, value });
            }
            // finally the parameter checks become `Some` and makes its way
            // into the new event structure: the list is formally correct, but
            // it may not be compatible with the signal parameters, in which
            // case the parameter check will evaluate to _non-verified_ and a
            // warning log message will be issued (see below)
            new_event.param_checks = Some(param_checks);

            // `parameter_check_all` only makes sense if the paramenter check
            // list was built: for this reason it is set only in this case
            // the enclosing `if` is that `Some(item) = cfgmap.get(cur_key)`
            // where `cur_key` is `"parameter_check"`
            let cur_key = "parameter_check_all";
            if let Some(item) = cfgmap.get(cur_key) {
                if !item.is_bool() {
                    return _invalid_cfg(
                        cur_key,
                        STR_UNKNOWN_VALUE,
                        ERR_INVALID_PARAMETER);
                } else {
                    new_event.param_checks_all = *item.as_bool().unwrap();
                }
            }

        }

        Ok(new_event)
    }

}


impl Event for DbusMessageEvent {

    fn set_id(&mut self, id: i64) { self.event_id = id; }
    fn get_name(&self) -> String { self.event_name.clone() }
    fn get_id(&self) -> i64 { self.event_id }

    fn requires_thread(&self) -> bool { true }  // maybe false, let's see

    fn get_condition(&self) -> Option<String> { self.condition_name.clone() }

    fn set_condition_registry(&mut self, reg: &'static ConditionRegistry) {
        self.condition_registry = Some(reg);
    }

    fn condition_registry(&self) -> Option<&'static ConditionRegistry> {
        self.condition_registry
    }

    fn set_condition_bucket(&mut self, bucket: &'static ExecutionBucket) {
        self.condition_bucket = Some(bucket);
    }

    fn condition_bucket(&self) -> Option<&'static ExecutionBucket> {
        self.condition_bucket
    }

    fn _assign_condition(&mut self, cond_name: &str) {
        // correctness has already been checked by the caller
        self.condition_name = Some(String::from(cond_name));
    }


    fn _start_service(&self) -> std::io::Result<bool> {

        // NOTE: the following helpers are async here, but since this service
        //       runs in a dedicated thread, we will just block on every step;
        //       the zbus::blocking option might be considered for further
        //       developement as well as rebuilding this service as real async

        // a helper to apply a given operator to two values without clutter;
        // for simplicity sake the `Match` operator will just evaluate to
        // `false` here, instead of generating an error: the `Err()` case would
        // clutter the code for numerical comparisons uslessly, as we also know
        // that the test are built only via `load_cfgmap`, and that it only
        // admits 'match' for regular expressions; the `Contains` operator also
        // evaluates to `false` here since this function only compares args
        // that are `PartialOrd+PartialEq`, and arrays are not
        fn _oper<T: PartialOrd+PartialEq>(op: &ParamCheckOperator, o1: T, o2: T) -> bool {
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
        fn _contained_in<T: Containable>(v: T, a: &zvariant::Value) -> bool {
            v.is_contained_in(a)
        }

        // panic here if the bus name is incorrect: should have been fixed
        // when the event was configured and constructed
        async fn _get_connection(bus: &str) -> zbus::Result<zbus::Connection> {
            let connection;
            if bus == ":session" {
                connection = zbus::Connection::session().await;
            } else if bus == ":system" {
                connection = zbus::Connection::system().await;
            } else {
                panic!("specified bus `{bus}` not supported for event");
            }
            connection
        }

        // provide the mesage stream we subscribed to through the filter; note
        // that the rule is moved here since this will be the only consumer
        async fn _get_stream(rule: zbus::MatchRule<'_>, conn: zbus::Connection) -> zbus::Result<zbus::MessageStream> {
            zbus::MessageStream::for_match_rule(rule, &conn, None).await
        }

        // first unwrap all data needed to install the listening service:
        // this panics if any of the data is uninitialized because all the
        // mandatory parameters must be set, and any missing value would
        // indicate that there is a mistake in the program flow
        let bus = self.bus
            .clone()
            .expect("attempt to start service with uninitialized bus");

        let rule_str = self.match_rule
            .clone()
            .expect("attempt to start service with uninitialized match rule");

        let rule = zbus::MatchRule::try_from(rule_str.as_str());
        if let Err(e) = rule {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("{ERR_EVENT_INVALID_MATCH_RULE}: {e}"),
            ));
        }
        let rule = rule.unwrap();

        // the loop
        let conn = task::block_on(async {
            self.log(
                LogType::Debug,
                LOG_WHEN_START,
                LOG_STATUS_MSG,
                &format!("opening connection to bus `{bus}`"),
            );
            _get_connection(&bus).await
        });
        if conn.is_err() {
            self.log(
                LogType::Warn,
                LOG_WHEN_START,
                LOG_STATUS_FAIL,
                &format!("could not establish connection on bus `{bus}`"),
            );
            return Ok(false);
        }
        let conn = conn.unwrap();

        let stream = task::block_on(async{
            self.log(
                LogType::Debug,
                LOG_WHEN_START,
                LOG_STATUS_MSG,
                &format!("opening message stream on bus `{bus}`"),
            );
            _get_stream(rule, conn).await
        });
        if stream.is_err() {
            self.log(
                LogType::Warn,
                LOG_WHEN_START,
                LOG_STATUS_FAIL,
                &format!("could not subscribe to message on bus `{bus}`"),
            );
            return Ok(false);
        }
        let mut stream = stream.unwrap();

        self.log(
            LogType::Debug,
            LOG_WHEN_START,
            LOG_STATUS_OK,
            &format!("successfully subscribed to message on bus `{bus}`"),
        );
        // FIXME: maybe implement [try_for_each](https://docs.rs/futures/latest/futures/stream/trait.TryStreamExt.html#method.try_for_each)
        // instead of continuously looping over the next item?
        loop {
            let msg = task::block_on(async {
                self.log(
                    LogType::Debug,
                    LOG_WHEN_PROC,
                    LOG_STATUS_MSG,
                    &format!("waiting for subscribed message on bus `{bus}`"),
                );
                stream.try_next().await
            });
            match msg {
                Ok(msg) => {
                    if let Some(message) = msg {
                        self.log(
                            LogType::Info,
                            LOG_WHEN_PROC,
                            LOG_STATUS_OK,
                            &format!("subscribed message received on bus `{bus}`"),
                        );
                        // check message parameters against provided criteria
                        // NOTE: any errors (bad indexes, invalid comparisons,
                        //       and such), cause the test to FAIL: this has
                        //       to be reported in the documentation
                        let mut verified = self.param_checks_all;

                        if let Some(checks) = &self.param_checks {
                            // if `param_checks` is not `None` the following code actually
                            // performs the required tests on the selected parameters
                            self.log(
                                LogType::Debug,
                                LOG_WHEN_PROC,
                                LOG_STATUS_MSG,
                                &format!(
                                    "parameter checks specified: {} checks must be verified",
                                    { if self.param_checks_all { "all" } else { "some" } },
                                ),
                            );
                            if let Ok(mbody) = message.body::<zvariant::Structure>() {
                                // the label is set to make sure that we can break out from
                                // any nested loop on shortcut evaluation condition (that is
                                // when all condition had to be true and at least one is false
                                // or when one true condition is sufficient and we find it)
                                // or when an error occurs, which implies evaluation to false
                                'params: for ck in checks.iter() {
                                    let argnum = ck.index.get(0);
                                    if let Some(argnum) = argnum {
                                        match argnum {
                                            ParameterIndex::Integer(x) => {
                                                if *x >= mbody.fields().len() as u64 {
                                                    self.log(
                                                        LogType::Warn,
                                                        LOG_WHEN_PROC,
                                                        LOG_STATUS_FAIL,
                                                        &format!("could not retrieve result: index {x} out of range"),
                                                    );
                                                    verified = false;
                                                    break 'params;
                                                }
                                                let mut field_value = mbody.fields().get(*x as usize).unwrap();
                                                for x in 1 .. ck.index.len() {
                                                    match ck.index.get(x).unwrap() {
                                                        ParameterIndex::Integer(i) => {
                                                            let i = *i as usize;
                                                            match field_value {
                                                                zvariant::Value::Array(f) => {
                                                                    if i >= f.len() {
                                                                        self.log(
                                                                            LogType::Warn,
                                                                            LOG_WHEN_PROC,
                                                                            LOG_STATUS_FAIL,
                                                                            &format!("could not retrieve result: index {i} out of range"),
                                                                        );
                                                                    }
                                                                    // if something is wrong here, either the test
                                                                    // or the next "parameter shift" will go berserk
                                                                    field_value = &f[i];
                                                                }
                                                                zvariant::Value::Structure(f) => {
                                                                    let f = f.fields();
                                                                    if i >= f.len() {
                                                                        self.log(
                                                                            LogType::Warn,
                                                                            LOG_WHEN_PROC,
                                                                            LOG_STATUS_FAIL,
                                                                            &format!("could not retrieve result: index {i} out of range"),
                                                                        );
                                                                    }
                                                                    if let Some(v) = f.get(i) {
                                                                        field_value = &v;
                                                                    } else {
                                                                        self.log(
                                                                            LogType::Warn,
                                                                            LOG_WHEN_PROC,
                                                                            LOG_STATUS_FAIL,
                                                                            &format!("could not retrieve result: index {i} provided no value"),
                                                                        );
                                                                    }
                                                                }
                                                                _ => {
                                                                    self.log(
                                                                        LogType::Warn,
                                                                        LOG_WHEN_PROC,
                                                                        LOG_STATUS_FAIL,
                                                                        &format!("could not retrieve result using index {i}"),
                                                                    );
                                                                    verified = false;
                                                                    break 'params;
                                                                }
                                                            }
                                                        }
                                                        ParameterIndex::String(s) => {
                                                            let s = s.as_str();
                                                            match field_value {
                                                                zvariant::Value::Dict(f) => {
                                                                    field_value = match f.get(s) {
                                                                        Ok(fv) => {
                                                                            if let Some(fv) = fv {
                                                                                fv
                                                                            } else {
                                                                                self.log(
                                                                                    LogType::Warn,
                                                                                    LOG_WHEN_PROC,
                                                                                    LOG_STATUS_FAIL,
                                                                                    &format!("could not retrieve result: index `{s}` invalid"),
                                                                                );
                                                                                verified = false;
                                                                                break 'params;
                                                                            }
                                                                        },
                                                                        Err(_) => {
                                                                            self.log(
                                                                                LogType::Warn,
                                                                                LOG_WHEN_PROC,
                                                                                LOG_STATUS_FAIL,
                                                                                &format!("could not retrieve result using index `{s}`"),
                                                                            );
                                                                            verified = false;
                                                                            break 'params;
                                                                        }
                                                                    }
                                                                }
                                                                _ => {
                                                                    self.log(
                                                                        LogType::Warn,
                                                                        LOG_WHEN_PROC,
                                                                        LOG_STATUS_FAIL,
                                                                        &format!("could not retrieve parameter using index `{s}`"),
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
                                                                        if !self.param_checks_all {
                                                                            break 'params;
                                                                        }
                                                                    } else {
                                                                        verified = false;
                                                                        if self.param_checks_all {
                                                                            break 'params;
                                                                        }
                                                                    }
                                                                }
                                                                e => {
                                                                    self.log(
                                                                        LogType::Warn,
                                                                        LOG_WHEN_PROC,
                                                                        LOG_STATUS_FAIL,
                                                                        &format!("mismatched result type: boolean expected (got `{e:?}`)"),
                                                                    );
                                                                    verified = false;
                                                                    break;
                                                                }
                                                            }
                                                        } else if ck.operator == ParamCheckOperator::Contains {
                                                            match field_value {
                                                                zvariant::Value::Array(_) => {
                                                                    if _contained_in(*b, field_value) {
                                                                        verified = true;
                                                                        if !self.param_checks_all {
                                                                            break 'params;
                                                                        }
                                                                    } else {
                                                                        verified = false;
                                                                        if self.param_checks_all {
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
                                                                    if !_contained_in(*b, field_value) {
                                                                        verified = true;
                                                                        if !self.param_checks_all {
                                                                            break 'params;
                                                                        }
                                                                    } else {
                                                                        verified = false;
                                                                        if self.param_checks_all {
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
                                                            self.log(
                                                                LogType::Warn,
                                                                LOG_WHEN_PROC,
                                                                LOG_STATUS_FAIL,
                                                                "invalid operator for boolean",
                                                            );
                                                            verified = false;
                                                            break;
                                                        }
                                                    }
                                                    ParameterCheckValue::Integer(i) => {
                                                        match field_value {
                                                            zvariant::Value::U8(v) => {
                                                                if _oper(&ck.operator, *i, *v as i64) {
                                                                    verified = true;
                                                                    if !self.param_checks_all {
                                                                        break 'params;
                                                                    }
                                                                } else {
                                                                    verified = false;
                                                                    if self.param_checks_all {
                                                                        break 'params;
                                                                    }
                                                                }
                                                            }
                                                            zvariant::Value::I16(v) => {
                                                                if _oper(&ck.operator, *i, *v as i64) {
                                                                    verified = true;
                                                                    if !self.param_checks_all {
                                                                        break 'params;
                                                                    }
                                                                } else {
                                                                    verified = false;
                                                                    if self.param_checks_all {
                                                                        break 'params;
                                                                    }
                                                                }
                                                            }
                                                            zvariant::Value::U16(v) => {
                                                                if _oper(&ck.operator, *i, *v as i64) {
                                                                    verified = true;
                                                                    if !self.param_checks_all {
                                                                        break 'params;
                                                                    }
                                                                } else {
                                                                    verified = false;
                                                                    if self.param_checks_all {
                                                                        break 'params;
                                                                    }
                                                                }
                                                            }
                                                            zvariant::Value::I32(v) => {
                                                                if _oper(&ck.operator, *i, *v as i64) {
                                                                    verified = true;
                                                                    if !self.param_checks_all {
                                                                        break 'params;
                                                                    }
                                                                } else {
                                                                    verified = false;
                                                                    if self.param_checks_all {
                                                                        break 'params;
                                                                    }
                                                                }
                                                            }
                                                            zvariant::Value::U32(v) => {
                                                                if _oper(&ck.operator, *i, *v as i64) {
                                                                    verified = true;
                                                                    if !self.param_checks_all {
                                                                        break 'params;
                                                                    }
                                                                } else {
                                                                    verified = false;
                                                                    if self.param_checks_all {
                                                                        break 'params;
                                                                    }
                                                                }
                                                            }
                                                            zvariant::Value::I64(v) => {
                                                                if _oper(&ck.operator, *i, *v as i64) {
                                                                    verified = true;
                                                                    if !self.param_checks_all {
                                                                        break 'params;
                                                                    }
                                                                } else {
                                                                    verified = false;
                                                                    if self.param_checks_all {
                                                                        break 'params;
                                                                    }
                                                                }
                                                            }
                                                            zvariant::Value::U64(v) => {
                                                                // lossy, however bigger numbers will just fail test
                                                                if _oper(&ck.operator, *i, *v as i64) {
                                                                    verified = true;
                                                                    if !self.param_checks_all {
                                                                        break 'params;
                                                                    }
                                                                } else {
                                                                    verified = false;
                                                                    if self.param_checks_all {
                                                                        break 'params;
                                                                    }
                                                                }
                                                            }
                                                            zvariant::Value::Array(_) => {
                                                                if ck.operator == ParamCheckOperator::Contains {
                                                                    if _contained_in(*i, field_value) {
                                                                        verified = true;
                                                                        if !self.param_checks_all {
                                                                            break 'params;
                                                                        }
                                                                    } else {
                                                                        verified = false;
                                                                        if self.param_checks_all {
                                                                            break 'params;
                                                                        }
                                                                    }
                                                                } else if ck.operator == ParamCheckOperator::NotContains {
                                                                    if !_contained_in(*i, field_value) {
                                                                        verified = true;
                                                                        if !self.param_checks_all {
                                                                            break 'params;
                                                                        }
                                                                    } else {
                                                                        verified = false;
                                                                        if self.param_checks_all {
                                                                            break 'params;
                                                                        }
                                                                    }
                                                                } else {
                                                                    verified = false;
                                                                    break 'params;
                                                                }
                                                            }
                                                            e => {
                                                                self.log(
                                                                    LogType::Warn,
                                                                    LOG_WHEN_PROC,
                                                                    LOG_STATUS_FAIL,
                                                                    &format!(
                                                                        "mismatched result type: {} expected (got `{e:?}`)",
                                                                        if ck.operator == ParamCheckOperator::Contains
                                                                            || ck.operator == ParamCheckOperator::NotContains { "container" }
                                                                        else { "integer" },
                                                                    ),
                                                                );
                                                                verified = false;
                                                                break 'params;
                                                            }
                                                        }
                                                    }
                                                    ParameterCheckValue::Float(f) => {
                                                        match field_value {
                                                            zvariant::Value::F64(v) => {
                                                                if _oper(&ck.operator, *f, *v) {
                                                                    verified = true;
                                                                    if !self.param_checks_all {
                                                                        break 'params;
                                                                    }
                                                                } else {
                                                                    verified = false;
                                                                    if self.param_checks_all {
                                                                        break 'params;
                                                                    }
                                                                }
                                                            }
                                                            zvariant::Value::Array(_) => {
                                                                if ck.operator == ParamCheckOperator::Contains {
                                                                    if _contained_in(*f, field_value) {
                                                                        verified = true;
                                                                        if !self.param_checks_all {
                                                                            break 'params;
                                                                        }
                                                                    } else {
                                                                        verified = false;
                                                                        if self.param_checks_all {
                                                                            break 'params;
                                                                        }
                                                                    }
                                                                } else if ck.operator == ParamCheckOperator::NotContains {
                                                                    if !_contained_in(*f, field_value) {
                                                                        verified = true;
                                                                        if !self.param_checks_all {
                                                                            break 'params;
                                                                        }
                                                                    } else {
                                                                        verified = false;
                                                                        if self.param_checks_all {
                                                                            break 'params;
                                                                        }
                                                                    }
                                                                } else {
                                                                    verified = false;
                                                                    break 'params;
                                                                }
                                                            }
                                                            e => {
                                                                self.log(
                                                                    LogType::Warn,
                                                                    LOG_WHEN_PROC,
                                                                    LOG_STATUS_FAIL,
                                                                    &format!(
                                                                        "mismatched result type: {} expected (got `{e:?}`)",
                                                                        if ck.operator == ParamCheckOperator::Contains
                                                                            || ck.operator == ParamCheckOperator::NotContains { "container" }
                                                                        else { "float" },
                                                                    ),
                                                                );
                                                                verified = false;
                                                                break 'params;
                                                            }
                                                        }
                                                    }
                                                    ParameterCheckValue::String(s) => {
                                                        if ck.operator == ParamCheckOperator::Equal {
                                                            match field_value {
                                                                zvariant::Value::Str(v) => {
                                                                    if *s == v.to_string() {
                                                                        verified = true;
                                                                        if !self.param_checks_all {
                                                                            break 'params;
                                                                        }
                                                                    } else {
                                                                        verified = false;
                                                                        if self.param_checks_all {
                                                                            break 'params;
                                                                        }
                                                                    }
                                                                }
                                                                zvariant::Value::ObjectPath(v) => {
                                                                    if *s == v.to_string() {
                                                                        verified = true;
                                                                        if !self.param_checks_all {
                                                                            break 'params;
                                                                        }
                                                                    } else {
                                                                        verified = false;
                                                                        if self.param_checks_all {
                                                                            break 'params;
                                                                        }
                                                                    }
                                                                }
                                                                e => {
                                                                    self.log(
                                                                        LogType::Warn,
                                                                        LOG_WHEN_PROC,
                                                                        LOG_STATUS_FAIL,
                                                                        &format!("mismatched result type: string expected (got `{e:?}`)"),
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
                                                                        if !self.param_checks_all {
                                                                            break 'params;
                                                                        }
                                                                    } else {
                                                                        verified = false;
                                                                        if self.param_checks_all {
                                                                            break 'params;
                                                                        }
                                                                    }
                                                                }
                                                                zvariant::Value::ObjectPath(v) => {
                                                                    if *s != v.to_string() {
                                                                        verified = true;
                                                                        if !self.param_checks_all {
                                                                            break 'params;
                                                                        }
                                                                    } else {
                                                                        verified = false;
                                                                        if self.param_checks_all {
                                                                            break 'params;
                                                                        }
                                                                    }
                                                                }
                                                                e => {
                                                                    self.log(
                                                                        LogType::Warn,
                                                                        LOG_WHEN_PROC,
                                                                        LOG_STATUS_FAIL,
                                                                        &format!("mismatched result type: string expected (got `{e:?}`)"),
                                                                    );
                                                                    verified = false;
                                                                    break 'params;
                                                                }
                                                            }
                                                        } else if ck.operator == ParamCheckOperator::Contains {
                                                            if _contained_in(s.clone(), field_value) {
                                                                verified = true;
                                                                if !self.param_checks_all {
                                                                    break 'params;
                                                                }
                                                            } else {
                                                                verified = false;
                                                                if self.param_checks_all {
                                                                    break 'params;
                                                                }
                                                            }
                                                        } else if ck.operator == ParamCheckOperator::NotContains {
                                                            if !_contained_in(s.clone(), field_value) {
                                                                verified = true;
                                                                if !self.param_checks_all {
                                                                    break 'params;
                                                                }
                                                            } else {
                                                                verified = false;
                                                                if self.param_checks_all {
                                                                    break 'params;
                                                                }
                                                            }
                                                        } else {
                                                            self.log(
                                                                LogType::Warn,
                                                                LOG_WHEN_PROC,
                                                                LOG_STATUS_FAIL,
                                                                "invalid operator for string",
                                                            );
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
                                                                        if !self.param_checks_all {
                                                                            break 'params;
                                                                        }
                                                                    } else {
                                                                        verified = false;
                                                                        if self.param_checks_all {
                                                                            break 'params;
                                                                        }
                                                                    }
                                                                }
                                                                zvariant::Value::ObjectPath(v) => {
                                                                    if re.is_match(v.as_str()) {
                                                                        verified = true;
                                                                        if !self.param_checks_all {
                                                                            break 'params;
                                                                        }
                                                                    } else {
                                                                        verified = false;
                                                                        if self.param_checks_all {
                                                                            break 'params;
                                                                        }
                                                                    }
                                                                }
                                                                e => {
                                                                    self.log(
                                                                        LogType::Warn,
                                                                        LOG_WHEN_PROC,
                                                                        LOG_STATUS_FAIL,
                                                                        &format!("mismatched result type: string expected (got `{e:?}`)"),
                                                                    );
                                                                    verified = false;
                                                                    break 'params;
                                                                }
                                                            }
                                                        } else {
                                                            self.log(
                                                                LogType::Warn,
                                                                LOG_WHEN_PROC,
                                                                LOG_STATUS_FAIL,
                                                                "invalid operator for regular expression",
                                                            );
                                                            verified = false;
                                                            break 'params;
                                                        }
                                                    }
                                                }
                                            }
                                            _ => {
                                                self.log(
                                                    LogType::Warn,
                                                    LOG_WHEN_PROC,
                                                    LOG_STATUS_FAIL,
                                                    "could not retrieve parameter index: no integer found",
                                                );
                                                verified = false;
                                                break;
                                            }
                                        }

                                    } else {
                                        self.log(
                                            LogType::Warn,
                                            LOG_WHEN_PROC,
                                            LOG_STATUS_FAIL,
                                            "could not retrieve parameter: missing argument number",
                                        );
                                        verified = false;
                                        break;
                                    }

                                }
                            } else {
                                self.log(
                                    LogType::Warn,
                                    LOG_WHEN_PROC,
                                    LOG_STATUS_FAIL,
                                    "could not retrieve message body",
                                );
                            }

                        } else {
                            // otherwise no parameters have been specified in
                            // the configuration file, check always positive
                            verified = true;
                        }

                        if verified {
                            match self.fire_condition() {
                                Ok(res) => {
                                    if res {
                                        self.log(
                                            LogType::Debug,
                                            LOG_WHEN_PROC,
                                            LOG_STATUS_OK,
                                            "condition fired successfully",
                                        );
                                    } else {
                                        self.log(
                                            LogType::Debug,
                                            LOG_WHEN_PROC,
                                            LOG_STATUS_MSG,
                                            "condition already fired: further schedule skipped",
                                        );
                                    }
                                }
                                Err(e) => {
                                    self.log(
                                        LogType::Warn,
                                        LOG_WHEN_PROC,
                                        LOG_STATUS_FAIL,
                                        &format!("error firing condition: {e}"),
                                    );
                                }
                            }
                        } else {
                            self.log(
                                LogType::Debug,
                                LOG_WHEN_PROC,
                                LOG_STATUS_MSG,
                                "parameter check failed: condition NOT fired",
                            );
                        }
                    } else {
                        // in normal conditions this is `unreachable!()`
                        self.log(
                            LogType::Debug,
                            LOG_WHEN_PROC,
                            LOG_STATUS_MSG,
                            &format!("no messages on bus `{bus}`: exiting"),
                        );
                        // close the stream before shutting down
                        let _ = task::block_on(async {
                            stream.async_drop().await
                        });
                        break;
                    }
                }
                Err(e) => {
                    self.log(
                        LogType::Warn,
                        LOG_WHEN_PROC,
                        LOG_STATUS_FAIL,
                        &format!("error while retrieving message on bus `{bus}`: {e}"),
                    );
                }
            }
        }

        self.log(
            LogType::Debug,
            LOG_WHEN_END,
            LOG_STATUS_OK,
            &format!("closing event listening service on bus `{bus}`"),
        );
        Ok(true)
    }

}


// end.
