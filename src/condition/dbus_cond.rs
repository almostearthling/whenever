//! Define a DBus method invocation based condition
//!
//! This type of condition is verified whenever an invocation to a provided
//! DBus method return a value that meets the criteria specified in the
//! configuration. The difference with the event based process consists in
//! the condition actively requesting DBus for a result.


use std::time::{Instant, Duration};
use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};

use cfgmap::{CfgMap, CfgValue};
use regex::Regex;

use async_std::task;
use zbus;
use zbus::zvariant;

use std::convert::TryFrom;
use std::str::FromStr;
use serde_json::value::Value;

use super::base::Condition;
use crate::task::registry::TaskRegistry;
use crate::common::logging::{log, LogType};
use crate::{cfg_mandatory, constants::*};

use crate::cfghelp::*;


// see the DBus specification
const DBUS_MAX_NUMBER_OF_ARGUMENTS: i64 = 63;


// an enum to store the operators for checking signal parameters
#[derive(PartialEq, Hash)]
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

// an enum containing the value that the parameter should be checked against
enum ParameterCheckValue {
    Boolean(bool),
    Integer(i64),
    Float(f64),
    String(String),
    Regex(Regex),
}

// an enum containing the possible types of indexes for parameters
#[derive(Hash)]
enum ParameterIndex {
    Integer(u64),
    String(String),
}

// a struct containing a single test to be performed against a signal payload
//
// short explaination, so that I remember how to use it:
// - `Index`: contains a list of indexes which specify, also for nested
//            structures. This means that for an array of mappings it might
//            be of the form `{ 1, 3, "somepos" }` where the first `1` is the
//            argument index, the `3` is the array index, and `"somepos"` is
//            the mapping index.
// - `Operator`: the operator to check the payload against
// - `Value`: the value to compare the parameter entry to
struct ParameterCheckTest {
    index: Vec<ParameterIndex>,
    operator: ParamCheckOperator,
    value: ParameterCheckValue,
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


// the trait used to convert values to `zvariant::Value`
trait ToVariant {
    fn to_variant(&self) -> Option<zvariant::Value>;
}

impl ToVariant for bool {
    fn to_variant(&self) -> Option<zvariant::Value> {
        Some(zvariant::Value::Bool(*self))
    }
}

impl ToVariant for i64 {
    fn to_variant(&self) -> Option<zvariant::Value> {
        Some(zvariant::Value::I64(*self))
    }
}

impl ToVariant for f64 {
    fn to_variant(&self) -> Option<zvariant::Value> {
        Some(zvariant::Value::F64(*self))
    }
}

impl ToVariant for str {
    fn to_variant(&self) -> Option<zvariant::Value> {
        let s = &self.to_string();
        if s.starts_with('\\') {
            let rest = s.clone().split_off(2);
            if s.starts_with("\\b") {
                let rest = rest.to_lowercase();
                if rest == "true" || rest == "1" {
                    return Some(zvariant::Value::Bool(true));
                } else if rest == "false" || rest == "0" {
                    return Some(zvariant::Value::Bool(false));
                } else {
                    return None;
                }
            } else if s.starts_with("\\y") {
                if let Ok(v) = rest.parse::<u8>() {
                    return Some(zvariant::Value::U8(v));
                } else {
                    return None;
                }
            } else if s.starts_with("\\n") {
                if let Ok(v) = rest.parse::<i16>() {
                    return Some(zvariant::Value::I16(v));
                } else {
                    return None;
                }
            } else if s.starts_with("\\q") {
                if let Ok(v) = rest.parse::<u16>() {
                    return Some(zvariant::Value::U16(v));
                } else {
                    return None;
                }
            } else if s.starts_with("\\i") {
                if let Ok(v) = rest.parse::<i32>() {
                    return Some(zvariant::Value::I32(v));
                } else {
                    return None;
                }
            } else if s.starts_with("\\u") {
                if let Ok(v) = rest.parse::<u32>() {
                    return Some(zvariant::Value::U32(v));
                } else {
                    return None;
                }
            } else if s.starts_with("\\x") {
                if let Ok(v) = rest.parse::<i64>() {
                    return Some(zvariant::Value::I64(v));
                } else {
                    return None;
                }
            } else if s.starts_with("\\t") {
                if let Ok(v) = rest.parse::<u64>() {
                    return Some(zvariant::Value::U64(v));
                } else {
                    return None;
                }
            } else if s.starts_with("\\d") {
                if let Ok(v) = rest.parse::<f64>() {
                    return Some(zvariant::Value::F64(v));
                } else {
                    return None;
                }
            } else if s.starts_with("\\s") {
                Some(zvariant::Value::new(rest.clone()))
            } else if s.starts_with("\\o") {
                // here we check it, having the RE at hand
                if RE_DBUS_OBJECT_PATH.is_match(&rest) {
                    Some(zvariant::Value::new(
                        zvariant::ObjectPath::try_from(rest.clone()).unwrap()))
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
where T: ToVariant {
    fn to_variant(&self) -> Option<zvariant::Value> {
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
where T: ToVariant {
    fn to_variant(&self) -> Option<zvariant::Value> {
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
    fn to_variant(&self) -> Option<zvariant::Value> {
        Some(self.clone())
    }
}

// and finally we support CfgValue, which is similar to a variant
impl ToVariant for CfgValue {
    fn to_variant(&self) -> Option<zvariant::Value> {
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
                        return None
                    }
                } else {
                    return None
                }
            }
            Some(zvariant::Value::new(h))
        } else {
            None
        }
    }
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

// the following is totally arbitrary and will actually not be used: it is
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



/// DBus Method Based Condition
///
/// This condition is verified whenever a value returned by a DBus method
/// invocation meets the criteria specified in the configuration.
pub struct DbusMethodCondition {
    // commom members
    // parameters
    cond_id: i64,
    cond_name: String,
    task_names: Vec<String>,
    recurring: bool,
    exec_sequence: bool,
    break_on_failure: bool,
    break_on_success: bool,
    suspended: bool,

    // internal values
    has_succeeded: bool,
    last_tested: Option<Instant>,
    last_succeeded: Option<Instant>,
    startup_time: Option<Instant>,
    task_registry: Option<&'static TaskRegistry>,

    // specific members
    // parameters
    bus: Option<String>,
    service: Option<String>,
    object_path: Option<String>,
    interface: Option<String>,
    method: Option<String>,
    param_call: Option<Vec<zvariant::OwnedValue>>,
    param_checks: Option<Vec<ParameterCheckTest>>,
    param_checks_all: bool,
    check_after: Option<Duration>,

    // internal values
    check_last: Instant,
}


// implement the hash protocol
impl Hash for DbusMethodCondition {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // common part
        self.cond_name.hash(state);
        self.recurring.hash(state);
        self.exec_sequence.hash(state);
        self.break_on_failure.hash(state);
        self.break_on_success.hash(state);
        // suspended is more a status: let's not consider it yet
        // self.suspended.hash(state);
        // task order is significant: hash on vec is not sorted
        self.task_names.hash(state);

        // specific part
        self.param_checks_all.hash(state);

        // 0 is hashed on the else branch in order to avoid that adjacent
        // strings one of which is undefined allow for hash collisions
        if let Some(x) = &self.bus {
            x.hash(state);
        } else {
            0.hash(state);
        }
        if let Some(x) = &self.service {
            x.hash(state);
        } else {
            0.hash(state);
        }
        if let Some(x) = &self.object_path {
            x.hash(state);
        } else {
            0.hash(state);
        }
        if let Some(x) = &self.interface {
            x.hash(state);
        } else {
            0.hash(state);
        }
        if let Some(x) = &self.method {
            x.hash(state);
        } else {
            0.hash(state);
        }

        // let's hope that to_string is a correct representation of the
        if let Some(x) = &self.param_call {
            for elem in x{
                elem.to_string().hash(state);
            }
        } else {
            0.hash(state);
        }

        self.param_checks.hash(state);
        self.param_checks_all.hash(state);
        self.check_after.hash(state);
    }
}


#[allow(dead_code)]
impl DbusMethodCondition {

    /// Create a new DBus method invocation based condition with the given
    /// name and interval duration
    pub fn new(
        name: &str,
    ) -> Self {
        log(
            LogType::Debug,
            LOG_EMITTER_CONDITION_DBUS,
            LOG_ACTION_NEW,
            Some((name, 0)),
            LOG_WHEN_INIT,
            LOG_STATUS_MSG,
            &format!("CONDITION {name}: creating a new DBus method based condition"),
        );
        let t = Instant::now();
        DbusMethodCondition {
            // common members initialization
            // reset ID
            cond_id: 0,

            // parameters
            cond_name: String::from(name),
            task_names: Vec::new(),
            recurring: false,
            exec_sequence: true,
            break_on_failure: false,
            break_on_success: false,
            suspended: true,

            // internal values
            startup_time: None,
            last_tested: None,
            last_succeeded: None,
            has_succeeded: false,
            task_registry: None,

            // specific members initialization
            // parameters
            check_after: None,
            bus: None,
            service: None,
            object_path: None,
            interface: None,
            method: None,
            param_call: None,
            param_checks: None,
            param_checks_all: false,

            // specific members initialization
            check_last: t,
        }
    }

    // constructor modifiers
    /// Set the command execution to sequence or parallel
    pub fn execs_sequentially(mut self, yes: bool) -> Self {
        self.exec_sequence = yes;
        self
    }

    /// If true, *sequential* task execution will break on first success
    pub fn breaks_on_success(mut self, yes: bool) -> Self {
        self.break_on_success = yes;
        self
    }

    /// If true, *sequential* task execution will break on first failure
    pub fn breaks_on_failure(mut self, yes: bool) -> Self {
        self.break_on_failure = yes;
        self
    }

    /// If true, create a recurring condition
    pub fn repeats(mut self, yes: bool) -> Self {
        self.recurring = yes;
        self
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


    /// Set the service name to the provided value (checks for validity)
    pub fn set_service(&mut self, name: &str) -> bool {
        if RE_DBUS_SERVICE_NAME.is_match(name) {
            self.service = Some(String::from(name));
            return true;
        }
        false
    }

    /// Return an owned copy of the service name
    pub fn service(&self) -> Option<String> { self.service.clone() }


    /// Set the object path to the provided value (checks for validity)
    pub fn set_object_path(&mut self, name: &str) -> bool {
        if RE_DBUS_OBJECT_PATH.is_match(name) {
            self.object_path = Some(String::from(name));
            return true;
        }
        false
    }

    /// Return an owned copy of the object path
    pub fn object_path(&self) -> Option<String> { self.object_path.clone() }


    /// Set the interface name to the provided value (checks for validity)
    pub fn set_interface(&mut self, name: &str) -> bool {
        if RE_DBUS_INTERFACE_NAME.is_match(name) {
            self.interface = Some(String::from(name));
            return true;
        }
        false
    }

    /// Return an owned copy of the interface name
    pub fn interface(&self) -> Option<String> { self.interface.clone() }


    /// Load a `DbusMethodCondition` from a [`CfgMap`](https://docs.rs/cfgmap/latest/)
    ///
    /// The `DbusMethodCondition` is initialized according to the values
    /// provided in the `CfgMap` argument. If the `CfgMap` format does not
    /// comply with the requirements of a `DbusMethodCondition` an error is
    /// raised.
    ///
    /// Note that the values for the `parameter_check` and `parameter_call`
    /// entries are provided as JSON strings, because TOML is intentionally
    /// limited to accepting only lists of elements of the same type, and in
    /// our case we need to mix types both as arguments to a call and as index
    /// sequences.
    ///
    /// The TOML configuration file format is the following
    ///
    /// ```toml
    /// # definition (mandatory)
    /// [[condition]]
    /// name = "DbusMethodConditionName"
    /// type = "dbus"                       # mandatory value
    /// bus = ":session"                    # either ":session" or ":system"
    /// service = "org.freedesktop.DBus"
    /// object_path = "/org/freedesktop/DBus"
    /// interface = "org.freedesktop.DBus"
    /// method = "NameHasOwner"
    ///
    /// # optional parameters (if omitted, defaults are used)
    /// recurring = false
    /// execute_sequence = true
    /// break_on_failure = false
    /// break_on_success = false
    /// suspended = true
    /// tasks = [ "Task1", "Task2", ... ]
    /// check_after = 60
    ///
    /// parameter_call = """[
    ///         "SomeObject",
    ///         [42, "a structured parameter"],
    ///         ["the following is an u64", "\\t42"]
    ///     ]"""
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
    /// Normally the JSON->DBUS conversion is performed as follows when
    /// interpreting the `parameter_call` entry:
    ///
    /// - Boolean --> Boolean
    /// - Integer --> I64
    /// - Float --> F64
    /// - String --> String
    /// - List --> Array
    /// - Map --> Dictionary (_string_ keyed!)
    ///
    /// Values of types not directly converted can be provided, in order to
    /// comply with signature, as strings (generally _literal_) prefixed with
    /// a backslash, immediately followed by the signature character, as in
    /// https://dbus.freedesktop.org/doc/dbus-specification.html#basic-types,
    /// and then the value to convert filling the string itself. This actually
    /// yields for all _basic_ types. A double backslash is interpreted as a
    /// backslash, and characters not specifying a basic type cause the string
    /// to be interpreted literally: '\u42' is thus 42u32, and '\w100' is the
    /// string "\w100" (including the backslash). Dictionaries with non-string
    /// keys are _not_ supported.
    ///
    /// Any incorrect value will cause an error. The value of the `type` entry
    /// *must* be set to `"dbus"` mandatorily for this type of `Condition`.
    pub fn load_cfgmap(cfgmap: &CfgMap, task_registry: &'static TaskRegistry) -> std::io::Result<DbusMethodCondition> {

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

        let check = vec![
            "type",
            "name",
            "tags",
            "interval_seconds",
            "tasks",
            "recurring",
            "execute_sequence",
            "break_on_failure",
            "break_on_success",
            "suspended",
            "check_after",
            "bus",
            "service",
            "object_path",
            "interface",
            "method",
            "parameter_call",
            "parameter_check_all",
            "parameter_check",
        ];
        cfg_check_keys(cfgmap, &check)?;

        // common mandatory parameter retrieval

        // type and name are both mandatory but type is only checked
        cfg_mandatory!(cfg_string_check_exact(cfgmap, "type", "dbus"))?;
        let name = cfg_mandatory!(cfg_string_check_regex(cfgmap, "name", &RE_COND_NAME))?.unwrap();

        // specific mandatory parameter retrieval
        let bus = cfg_mandatory!(cfg_string_check_regex(cfgmap, "bus", &RE_DBUS_MSGBUS_NAME))?.unwrap();
        let service = cfg_mandatory!(cfg_string_check_regex(cfgmap, "service", &RE_DBUS_SERVICE_NAME))?.unwrap();
        let object_path = cfg_mandatory!(cfg_string_check_regex(cfgmap, "object_path", &RE_DBUS_OBJECT_PATH))?.unwrap();
        let interface = cfg_mandatory!(cfg_string_check_regex(cfgmap, "interface", &RE_DBUS_INTERFACE_NAME))?.unwrap();
        let method = cfg_mandatory!(cfg_string_check_regex(cfgmap, "method", &RE_DBUS_MEMBER_NAME))?.unwrap();

        // initialize the structure
        let mut new_condition = DbusMethodCondition::new(
            &name,
        );
        new_condition.task_registry = Some(task_registry);
        new_condition.bus = Some(bus);
        new_condition.service = Some(service);
        new_condition.object_path = Some(object_path);
        new_condition.interface = Some(interface);
        new_condition.method = Some(method);

        // by default make condition active if loaded from configuration: if
        // the configuration changes this state the condition will not start
        new_condition.suspended = false;

        // common optional parameter initialization

        // tags are always simply checked this way as no value is needed
        let cur_key = "tags";
        if let Some(item) = cfgmap.get(cur_key) {
            if !item.is_list() && !item.is_map() {
                return Err(cfg_err_invalid_config(
                    cur_key,
                    STR_UNKNOWN_VALUE,
                    ERR_INVALID_PARAMETER,
                ));
            }
        }

        // retrieve task list and try to directly add each task
        if let Some(v) = cfg_vec_string_check_regex(cfgmap, "tasks", &RE_TASK_NAME)? {
            for s in v {
                if !new_condition.add_task(&s)? {
                    return Err(cfg_err_invalid_config(
                        cur_key,
                        &s,
                        ERR_INVALID_TASK,
                    ));
                }
            }
        }

        if let Some(v) = cfg_bool(cfgmap, "recurring")? {
            new_condition.recurring = v;
        }
        if let Some(v) = cfg_bool(cfgmap, "execute_sequence")? {
            new_condition.exec_sequence = v;
        }
        if let Some(v) = cfg_bool(cfgmap, "break_on_failure")? {
            new_condition.break_on_failure = v;
        }
        if let Some(v) = cfg_bool(cfgmap, "break_on_success")? {
            new_condition.break_on_success = v;
        }
        if let Some(v) = cfg_bool(cfgmap, "suspended")? {
            new_condition.suspended = v;
        }

        // specific optional parameter initialization
        if let Some(v) = cfg_int_check_above_eq(cfgmap, "check_after", 1)? {
            new_condition.check_after = Some(Duration::from_secs(v as u64));
        }

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
                return Err(cfg_err_invalid_config(
                    cur_key,
                    STR_UNKNOWN_VALUE,
                    ERR_INVALID_VALUE_FOR_ENTRY,
                ));
            }
            // since CfgMap only accepts maps as input, and we expect a list
            // instead, we build a map with a single element labeled '0':
            let json = Value::from_str(
                &format!("{{\"0\": {}}}", item.as_str().unwrap())
            );
            if json.is_err() {
                return Err(cfg_err_invalid_config(
                    cur_key,
                    STR_UNKNOWN_VALUE,
                    ERR_INVALID_VALUE_FOR_ENTRY,
                ));
            }
            // and then we extract the '0' element and check it to be a list
            let item = CfgMap::from_json(json.unwrap());
            let item = item.get("0").unwrap();
            if !item.is_list() {
                return Err(cfg_err_invalid_config(
                    cur_key,
                    STR_UNKNOWN_VALUE,
                    ERR_INVALID_VALUE_FOR_ENTRY,
                ));
            }
            let item = item.as_list().unwrap();
            for spec in item.iter() {
                if !spec.is_map() {
                    return Err(cfg_err_invalid_config(
                        cur_key,
                        STR_UNKNOWN_VALUE,
                        ERR_INVALID_VALUE_FOR_ENTRY,
                    ));
                }
                let spec = spec.as_map().unwrap();
                for key in spec.keys() {
                    if !check.contains(&key.as_str()) {
                        return Err(cfg_err_invalid_config(
                            &format!("{cur_key}:{key}"),
                            STR_UNKNOWN_VALUE,
                            &format!("{ERR_INVALID_CFG_ENTRY} ({key})"),
                        ));
                    }
                }
                let mut index_list: Vec<ParameterIndex> = Vec::new();
                if let Some(index) = spec.get("index") {
                    if index.is_int() {
                        if let Some(px) = _check_dbus_param_index(index) {
                            index_list.push(px);
                        } else {
                            return Err(cfg_err_invalid_config(
                                &format!("{cur_key}:index"),
                                &format!("{index:?}"),
                                ERR_INVALID_VALUE_FOR_ENTRY,
                            ));
                        }
                    } else if index.is_list() {
                        for sub_index in index.as_list().unwrap() {
                            if let Some(px) = _check_dbus_param_index(sub_index) {
                                index_list.push(px);
                            } else {
                                return Err(cfg_err_invalid_config(
                                    &format!("{cur_key}:index"),
                                    &format!("{sub_index:?}"),
                                    ERR_INVALID_VALUE_FOR_ENTRY,
                                ));
                            }
                        }
                    } else {
                        return Err(cfg_err_invalid_config(
                            &format!("{cur_key}:index"),
                            &format!("{index:?}"),
                            ERR_INVALID_VALUE_FOR_ENTRY,
                        ));
                    }
                } else {
                    return Err(cfg_err_invalid_config(
                        &format!("{cur_key}:index"),
                        STR_UNKNOWN_VALUE,
                        ERR_MISSING_PARAMETER,
                    ));
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
                                return Err(cfg_err_invalid_config(
                                    &format!("{cur_key}:operator"),
                                    &format!("{oper:?}"),
                                    ERR_INVALID_VALUE_FOR_ENTRY,
                                ));
                            }
                        };
                    } else {
                        return Err(cfg_err_invalid_config(
                            &format!("{cur_key}:operator"),
                            STR_UNKNOWN_VALUE,
                            ERR_INVALID_VALUE_FOR_ENTRY,
                        ));
                    }
                } else {
                    return Err(cfg_err_invalid_config(
                        &format!("{cur_key}:operator"),
                        STR_UNKNOWN_VALUE,
                        ERR_MISSING_PARAMETER,
                    ));
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
                            let re = Regex::new(s);
                            if let Ok(re) = re {
                                value = ParameterCheckValue::Regex(re);
                            } else {
                                return Err(cfg_err_invalid_config(
                                    &format!("{cur_key}:value"),
                                    STR_UNKNOWN_VALUE,
                                    ERR_INVALID_VALUE_FOR_ENTRY,
                                ));
                            }
                        } else {
                            value = ParameterCheckValue::String(s.to_string());
                        }
                    } else {
                        return Err(cfg_err_invalid_config(
                            &format!("{cur_key}:value"),
                            STR_UNKNOWN_VALUE,
                            ERR_INVALID_VALUE_FOR_ENTRY,
                        ));
                    }
                } else {
                    return Err(cfg_err_invalid_config(
                        &format!("{cur_key}:value"),
                        STR_UNKNOWN_VALUE,
                        ERR_MISSING_PARAMETER,
                    ));
                }
                // now that we have the full triple, we can add it to criteria
                param_checks.push(ParameterCheckTest { index: index_list, operator, value });
            }
            // finally the parameter checks become `Some` and makes its way
            // into the new condition structure: the list is formally correct,
            // but it may not be compatible with the returned parameters, in
            // which case the parameter check will evaluate to _non-verified_
            // and a warning log message will be issued (see below)
            new_condition.param_checks = Some(param_checks);

            // `parameter_check_all` only makes sense if the paramenter check
            // list was built: for this reason it is set only in this case
            if let Some(v) = cfg_bool(cfgmap, "parameter_check_all")? {
                new_condition.param_checks_all = v;
            }
        }

        // here we must build a list of `zvariant::Value` objects, which are
        // dynamic: the list will be formally a valid parameter list but it is
        // not assured to be compatible with the called method (that is, no
        // check against signature is made here); in case of incompatibility
        // the condition evaluation will (always) fail and a warning will be
        // logged
        let cur_key = "parameter_call";
        if let Some(item) = cfgmap.get(cur_key) {
            let mut param_call: Vec<zvariant::OwnedValue> = Vec::new();
            // the process here is the same as the one for parameter checks:
            // here we expect a JSON string, reason explained above
            if !item.is_str() {
                return Err(cfg_err_invalid_config(
                    cur_key,
                    STR_UNKNOWN_VALUE,
                    ERR_INVALID_VALUE_FOR_ENTRY,
                ));
            }
            // since CfgMap only accepts maps as input, and we expect a list
            // instead, we build a map with a single element labeled '0':
            let json = Value::from_str(
                &format!("{{\"0\": {}}}", item.as_str().unwrap())
            );
            if json.is_err() {
                return Err(cfg_err_invalid_config(
                    cur_key,
                    STR_UNKNOWN_VALUE,
                    ERR_INVALID_VALUE_FOR_ENTRY,
                ));
            }
            // and then we extract the '0' element and check it to be a list
            let item = CfgMap::from_json(json.unwrap());
            let item = item.get("0").unwrap();
            if !item.is_list() {
                return Err(cfg_err_invalid_config(
                    cur_key,
                    STR_UNKNOWN_VALUE,
                    ERR_INVALID_VALUE_FOR_ENTRY,
                ));
            }
            let item = item.as_list().unwrap();
            // the `ToVariant` trait should do the tedious recursive job for
            // us: should there be any unsupported value in the array the
            // result will be None and the configuration is rejectesd
            for i in item.iter() {
                let v = i.to_variant();
                if let Some(v) = v {
                    param_call.push(v.into());
                } else {
                    return Err(cfg_err_invalid_config(
                        cur_key,
                        STR_UNKNOWN_VALUE,
                        ERR_INVALID_VALUE_FOR_ENTRY,
                    ));
                }
            }
            // the parameters for the message invocation can now be set
            new_condition.param_call = Some(param_call);
        }

        // start the condition if the configuration did not suspend it
        if !new_condition.suspended {
            new_condition.start()?;
        }

        Ok(new_condition)
    }
}



impl Condition for DbusMethodCondition {

    fn set_id(&mut self, id: i64) { self.cond_id = id; }
    fn get_name(&self) -> String { self.cond_name.clone() }
    fn get_id(&self) -> i64 { self.cond_id }
    fn get_type(&self) -> &str { "interval" }

    /// Return a hash of this item for comparison
    fn _hash(&self) -> u64 {
        let mut s = DefaultHasher::new();
        self.hash(&mut s);
        s.finish()
    }


    fn set_task_registry(&mut self, reg: &'static TaskRegistry) {
        self.task_registry = Some(reg);
    }

    fn task_registry(&self) -> Option<&'static TaskRegistry> {
        self.task_registry
    }


    fn suspended(&self) -> bool { self.suspended }
    fn recurring(&self) -> bool { self.recurring }
    fn has_succeeded(&self) -> bool { self.has_succeeded }

    fn exec_sequence(&self) -> bool { self.exec_sequence }
    fn break_on_success(&self) -> bool { self.break_on_success }
    fn break_on_failure(&self) -> bool { self.break_on_failure }

    fn last_checked(&self) -> Option<Instant> { self.last_tested }
    fn last_succeeded(&self) -> Option<Instant> { self.last_succeeded }
    fn startup_time(&self) -> Option<Instant> { self.startup_time }

    fn set_checked(&mut self) -> Result<bool, std::io::Error> {
        self.last_tested = Some(Instant::now());
        Ok(true)
    }

    fn set_succeeded(&mut self) -> Result<bool, std::io::Error> {
        self.last_succeeded = self.last_tested;
        self.has_succeeded = true;
        Ok(true)
    }

    fn reset_succeeded(&mut self) -> Result<bool, std::io::Error> {
        self.last_succeeded = None;
        self.has_succeeded = false;
        Ok(true)
    }

    fn reset(&mut self) -> Result<bool, std::io::Error> {
        self.last_tested = None;
        self.last_succeeded = None;
        self.has_succeeded = false;
        Ok(true)
    }


    fn start(&mut self) -> Result<bool, std::io::Error> {
        self.suspended = false;
        self.startup_time = Some(Instant::now());
        Ok(true)
    }

    fn suspend(&mut self) -> Result<bool, std::io::Error> {
        if self.suspended {
            Ok(false)
        } else {
            self.suspended = true;
            Ok(true)
        }
    }

    fn resume(&mut self) -> Result<bool, std::io::Error> {
        if self.suspended {
            self.suspended = false;
            Ok(true)
        } else {
            Ok(false)
        }
    }


    fn task_names(&self) -> Result<Vec<String>, std::io::Error> {
        Ok(self.task_names.clone())
    }


    fn _add_task(&mut self, name: &str) -> Result<bool, std::io::Error> {
        let name = String::from(name);
        if self.task_names.contains(&name) {
            Ok(false)
        } else {
            self.task_names.push(name);
            Ok(true)
        }
    }

    fn _remove_task(&mut self, name: &str) -> Result<bool, std::io::Error> {
        let name = String::from(name);
        if self.task_names.contains(&name) {
            self.task_names.remove(
                self.task_names
                .iter()
                .position(|x| x == &name)
                .unwrap());
            Ok(true)
        } else {
            Ok(false)
        }
    }


    /// Mandatory check function.
    ///
    /// This function actually performs the test.
    fn _check_condition(&mut self) -> Result<Option<bool>, std::io::Error> {

        // NOTE: the following helpers are async here, but since this check
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
        // when the condition was configured and constructed
        async fn _get_connection(bus: &str) -> zbus::Result<zbus::Connection> {
            let connection;
            if bus == ":session" {
                connection = zbus::Connection::session().await;
            } else if bus == ":system" {
                connection = zbus::Connection::system().await;
            } else {
                panic!("specified bus `{bus}` not supported for DBus method based condition");
            }
            connection
        }

        self.log(
            LogType::Debug,
            LOG_WHEN_START,
            LOG_STATUS_MSG,
            "checking DBus method based condition",
        );
        // if the minimum interval between checks has been set, obey it
        // last_tested has already been set by trait to Instant::now()
        let t = self.last_tested.unwrap();
        if let Some(e) = self.check_after {
            if e > t - self.check_last {
                self.log(
                    LogType::Debug,
                    LOG_WHEN_START,
                    LOG_STATUS_MSG,
                    "check explicitly delayed by configuration",
                );
                return Ok(Some(false));
            }
        }


        // first unwrap all data needed to install the listening service:
        // this panics if any of the data is uninitialized because all the
        // mandatory parameters must be set, and any missing value would
        // indicate that there is a mistake in the program flow
        let bus = self.bus
            .clone()
            .expect("attempt to check condition with uninitialized bus");

        let service = self.service
            .clone()
            .expect("attempt to check condition with uninitialized service");

        let object_path = self.object_path
            .clone()
            .expect("attempt to check condition with uninitialized object path");

        let interface = self.interface
            .clone()
            .expect("attempt to check condition with uninitialized interface");

        let method = self.method
            .clone()
            .expect("attempt to check condition with uninitialized method");

        // connect to the DBus service
        self.log(
            LogType::Debug,
            LOG_WHEN_START,
            LOG_STATUS_MSG,
            &format!("opening connection to bus `{bus}`"),
        );
        let conn = task::block_on(async {
            _get_connection(&bus).await
        });
        if conn.is_err() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotConnected,
                format!("{ERR_COND_CANNOT_CONNECT_TO} {bus}"),
            ));
        }
        let conn = conn.unwrap();

        // the following bifurcation is in my opinion quite ugly, however it
        // looks like the entire `zbus` crate is built with the purpose of
        // providing a way to proxy known existing DBus methods at compile
        // time, and thus I did not found a way to call a method without
        // arguments if not by passing a pointer to the unit as argument to
        // the `call_method` function
        let message;
        if let Some(params) = self.param_call.clone() {
            let mut arg = zvariant::StructureBuilder::new();
            for p in params {
                let v = zvariant::Value::from(p);
                arg.push_value(v);
            }
            message = task::block_on(async {
                conn.call_method(
                    if service.is_empty() { None } else { Some(service.as_str()) },
                    object_path.as_str(),
                    if interface.is_empty() { None } else { Some(interface.as_str()) },
                    method.as_str(),
                    &arg.build(),
                ).await
            });
        } else {
            message = task::block_on(async {
                conn.call_method(
                    if service.is_empty() { None } else { Some(service.as_str()) },
                    object_path.as_str(),
                    if interface.is_empty() { None } else { Some(interface.as_str()) },
                    method.as_str(),
                    &(),
                ).await
            });
        }
        if let Err(e) = message {
            self.log(
                LogType::Warn,
                LOG_WHEN_START,
                LOG_STATUS_FAIL,
                &format!("could not retrieve message invoking method {method} on bus `{bus}`: {e}"),
            );
            return Ok(Some(false));
        }

        // now check method result in the same way as in signal message
        // (see `_start_service` in `dbus_event` for details)
        let mut verified = self.param_checks_all;
        if let Some(checks) = &self.param_checks {
            if let Ok(mbody) = message.unwrap().clone().body::<zvariant::Structure>() {
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
            panic!("attempt to verify condition without initializing tests")
        }

        // now the time of the last check can be set to the actual time in
        // order to allow further checks to comply with the request to be
        // only run at certain intervals
        self.check_last = t;

        Ok(Some(verified))
    }

}


// end.
