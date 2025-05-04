// this utility may contain unused functions or other items
#![allow(dead_code)]

/// module cfghelp
///
/// module providing shortcut functions/macros to help configuration of items
/// by providing a CfgMap instance and the key to be retrieved
use cfgmap::CfgMap;
use regex::Regex;

use crate::common::wres::{Error, Kind, Result};
use crate::constants::*;

/// use this to specify that a configuration element is mandatory
///
/// Note: this macro is only intended to be used with the functions that are
/// defined in this package, as no check is made on what expression is used
/// as argument - at least for now
#[macro_export]
macro_rules! cfg_mandatory {
    ($func_name:ident($cfgmap:expr, $key:expr)) => {
        if let Some(v) = $func_name($cfgmap, $key)? {
            Ok(Some(v))
        } else {
            Err($crate::cfghelp::cfg_err_invalid_config(
                $key,
                STR_UNKNOWN_VALUE,
                ERR_MISSING_PARAMETER,
            ))
        }
    };
    ($func_name:ident($cfgmap:expr, $key:expr, $a1:expr)) => {
        if let Some(v) = $func_name($cfgmap, $key, $a1)? {
            Ok(Some(v))
        } else {
            Err(crate::cfghelp::cfg_err_invalid_config(
                $key,
                STR_UNKNOWN_VALUE,
                ERR_MISSING_PARAMETER,
            ))
        }
    };
    ($func_name:ident($cfgmap:expr, $key:expr, $a1:expr, $a2:expr)) => {
        if let Some(v) = $func_name($cfgmap, $key, $a1, $a2)? {
            Ok(Some(v))
        } else {
            Err(crate::cfghelp::cfg_err_invalid_config(
                $key,
                STR_UNKNOWN_VALUE,
                ERR_MISSING_PARAMETER,
            ))
        }
    };
}

/// build a suitable error to be returned for invalid configurations
pub fn cfg_err_invalid_config(key: &str, value: &str, message: &str) -> Error {
    Error::new(
        Kind::Invalid,
        &format!("{ERR_INVALID_ITEM_CONFIG}: ({key}={value}) {message}"),
    )
}

/// check that a configuration map only contains keys among the specified ones
pub fn cfg_check_keys(cfgmap: &CfgMap, check: &Vec<&str>) -> Result<()> {
    for key in cfgmap.keys() {
        if !check.contains(&key.as_str()) {
            return Err(cfg_err_invalid_config(
                key,
                STR_UNKNOWN_VALUE,
                &format!("{ERR_INVALID_CFG_ENTRY} ({key})"),
            ));
        }
    }
    Ok(())
}

/// get a boolean
pub fn cfg_bool(cfgmap: &CfgMap, key: &str) -> Result<Option<bool>> {
    if let Some(item) = cfgmap.get(key) {
        if !item.is_bool() {
            return Err(cfg_err_invalid_config(
                key,
                STR_UNKNOWN_VALUE,
                ERR_INVALID_PARAMETER,
            ));
        }
        Ok(Some(*item.as_bool().unwrap()))
    } else {
        Ok(None)
    }
}

/// get an integer without checks
pub fn cfg_int(cfgmap: &CfgMap, key: &str) -> Result<Option<i64>> {
    if let Some(item) = cfgmap.get(key) {
        if !item.is_int() {
            return Err(cfg_err_invalid_config(
                key,
                STR_UNKNOWN_VALUE,
                ERR_INVALID_PARAMETER,
            ));
        }
        Ok(Some(item.as_int().unwrap().to_owned()))
    } else {
        Ok(None)
    }
}

/// get an integer checking it with provided closure
pub fn cfg_int_check<F: Fn(i64) -> bool>(
    cfgmap: &CfgMap,
    key: &str,
    check: F,
) -> Result<Option<i64>> {
    if let Some(v) = cfg_int(cfgmap, key)? {
        if check(v) {
            Ok(Some(v))
        } else {
            Err(cfg_err_invalid_config(
                key,
                &format!("{v}"),
                ERR_INVALID_VALUE_FOR_ENTRY,
            ))
        }
    } else {
        Ok(None)
    }
}

/// get an integer in a specific interval (including boundaries `a` and `b`)
pub fn cfg_int_check_interval(cfgmap: &CfgMap, key: &str, a: i64, b: i64) -> Result<Option<i64>> {
    cfg_int_check(cfgmap, key, |x| a <= x && x <= b)
}

/// get an integer equal or above a certain value
pub fn cfg_int_check_above_eq(cfgmap: &CfgMap, key: &str, a: i64) -> Result<Option<i64>> {
    cfg_int_check(cfgmap, key, |x| x >= a)
}

/// get an integer equal or below a certain value
pub fn cfg_int_check_below_eq(cfgmap: &CfgMap, key: &str, a: i64) -> Result<Option<i64>> {
    cfg_int_check(cfgmap, key, |x| x <= a)
}

/// get an integer provided that it is exactly a certain value
pub fn cfg_int_check_eq(cfgmap: &CfgMap, key: &str, a: i64) -> Result<Option<i64>> {
    cfg_int_check(cfgmap, key, |x| x == a)
}

/// get a float without checks
pub fn cfg_float(cfgmap: &CfgMap, key: &str) -> Result<Option<f64>> {
    if let Some(item) = cfgmap.get(key) {
        if !item.is_float() {
            return Err(cfg_err_invalid_config(
                key,
                STR_UNKNOWN_VALUE,
                ERR_INVALID_PARAMETER,
            ));
        }
        Ok(Some(item.as_float().unwrap().to_owned()))
    } else {
        Ok(None)
    }
}

/// get a float checking it with provided closure
pub fn cfg_float_check<F: Fn(f64) -> bool>(
    cfgmap: &CfgMap,
    key: &str,
    check: F,
) -> Result<Option<f64>> {
    if let Some(v) = cfg_float(cfgmap, key)? {
        if check(v) {
            Ok(Some(v))
        } else {
            Err(cfg_err_invalid_config(
                key,
                &format!("{v}"),
                ERR_INVALID_VALUE_FOR_ENTRY,
            ))
        }
    } else {
        Ok(None)
    }
}

/// get a float in a specific interval (including boundaries `a` and `b`)
pub fn cfg_float_check_interval(cfgmap: &CfgMap, key: &str, a: f64, b: f64) -> Result<Option<f64>> {
    cfg_float_check(cfgmap, key, |x| a <= x && x <= b)
}

/// get a float equal or above a certain value
pub fn cfg_float_check_above_eq(cfgmap: &CfgMap, key: &str, a: f64) -> Result<Option<f64>> {
    cfg_float_check(cfgmap, key, |x| x >= a)
}

/// get a float equal or below a certain value
pub fn cfg_float_check_below_eq(cfgmap: &CfgMap, key: &str, a: f64) -> Result<Option<f64>> {
    cfg_float_check(cfgmap, key, |x| x <= a)
}

/// get a float provided that it is exactly a certain value
pub fn cfg_float_check_eq(cfgmap: &CfgMap, key: &str, a: f64) -> Result<Option<f64>> {
    cfg_float_check(cfgmap, key, |x| x == a)
}

/// get a string without checks
pub fn cfg_string(cfgmap: &CfgMap, key: &str) -> Result<Option<String>> {
    if let Some(item) = cfgmap.get(key) {
        if !item.is_str() {
            return Err(cfg_err_invalid_config(
                key,
                STR_UNKNOWN_VALUE,
                ERR_INVALID_PARAMETER,
            ));
        }
        Ok(Some(item.as_str().unwrap().to_owned()))
    } else {
        Ok(None)
    }
}

/// get a string checking it with provided closure
pub fn cfg_string_check<F: Fn(&str) -> bool>(
    cfgmap: &CfgMap,
    key: &str,
    check: F,
) -> Result<Option<String>> {
    if let Some(v) = cfg_string(cfgmap, key)? {
        if check(&v) {
            Ok(Some(v))
        } else {
            Err(cfg_err_invalid_config(
                key,
                v.as_str(),
                ERR_INVALID_VALUE_FOR_ENTRY,
            ))
        }
    } else {
        Ok(None)
    }
}

/// get a string checking it against a fixed string
pub fn cfg_string_check_exact(cfgmap: &CfgMap, key: &str, check: &str) -> Result<Option<String>> {
    cfg_string_check(cfgmap, key, |s| s == check)
}

/// get a string checking it against a fixed string ignoring case
pub fn cfg_string_check_exact_nocase(
    cfgmap: &CfgMap,
    key: &str,
    check: &str,
) -> Result<Option<String>> {
    cfg_string_check(cfgmap, key, |s| s.to_uppercase() == check.to_uppercase())
}

/// get a string provided that it is in a certain set of strings
pub fn cfg_string_check_within(
    cfgmap: &CfgMap,
    key: &str,
    check: &Vec<&str>,
) -> Result<Option<String>> {
    cfg_string_check(cfgmap, key, |x| check.contains(&x))
}

/// get a string provided that it is in a certain set of strings ignoring case
pub fn cfg_string_check_within_nocase(
    cfgmap: &CfgMap,
    key: &str,
    check: &Vec<&str>,
) -> Result<Option<String>> {
    // TODO: there is probably a less expensive way to do it
    let mut new_check: Vec<String> = Vec::new();
    for x in check {
        new_check.push(x.to_uppercase());
    }
    cfg_string_check(cfgmap, key, |x| new_check.contains(&x.to_uppercase()))
}

/// get a string checking it against a regular expression
pub fn cfg_string_check_regex(cfgmap: &CfgMap, key: &str, check: &Regex) -> Result<Option<String>> {
    cfg_string_check(cfgmap, key, |s| check.is_match(s))
}

/// get a list of booleans
pub fn cfg_vec_bool(cfgmap: &CfgMap, key: &str) -> Result<Option<Vec<bool>>> {
    if let Some(item) = cfgmap.get(key) {
        if !item.is_list() {
            return Err(cfg_err_invalid_config(
                key,
                STR_UNKNOWN_VALUE,
                ERR_INVALID_PARAMETER,
            ));
        }
        let mut v: Vec<bool> = Vec::new();
        for elem in item.as_list().unwrap() {
            if !elem.is_bool() {
                return Err(cfg_err_invalid_config(
                    key,
                    STR_UNKNOWN_VALUE,
                    ERR_INVALID_PARAMETER_LIST,
                ));
            } else {
                v.push(*elem.as_bool().unwrap());
            }
        }
        Ok(Some(v))
    } else {
        Ok(None)
    }
}

/// get a list of integers
pub fn cfg_vec_int(cfgmap: &CfgMap, key: &str) -> Result<Option<Vec<i64>>> {
    if let Some(item) = cfgmap.get(key) {
        if !item.is_list() {
            return Err(cfg_err_invalid_config(
                key,
                STR_UNKNOWN_VALUE,
                ERR_INVALID_PARAMETER,
            ));
        }
        let mut v: Vec<i64> = Vec::new();
        for elem in item.as_list().unwrap() {
            if !elem.is_int() {
                return Err(cfg_err_invalid_config(
                    key,
                    STR_UNKNOWN_VALUE,
                    ERR_INVALID_PARAMETER_LIST,
                ));
            } else {
                v.push(*elem.as_int().unwrap());
            }
        }
        Ok(Some(v))
    } else {
        Ok(None)
    }
}

/// get a list of integers checking all of them with provided closure
pub fn cfg_vec_int_check<F: Fn(i64) -> bool>(
    cfgmap: &CfgMap,
    key: &str,
    check: F,
) -> Result<Option<Vec<i64>>> {
    if let Some(v) = cfg_vec_int(cfgmap, key)? {
        for elem in v.iter() {
            if !check(*elem) {
                return Err(cfg_err_invalid_config(
                    key,
                    &format!("{elem}"),
                    ERR_INVALID_VALUE_FOR_LIST_ENTRY,
                ));
            }
        }
        Ok(Some(v))
    } else {
        Ok(None)
    }
}

/// get a list of integers in a specific interval (including boundaries `a` and `b`)
pub fn cfg_vec_int_check_interval(
    cfgmap: &CfgMap,
    key: &str,
    a: i64,
    b: i64,
) -> Result<Option<Vec<i64>>> {
    cfg_vec_int_check(cfgmap, key, |x| a <= x && x <= b)
}

/// get a list of integers equal or above a certain value
pub fn cfg_vec_int_check_above_eq(cfgmap: &CfgMap, key: &str, a: i64) -> Result<Option<Vec<i64>>> {
    cfg_vec_int_check(cfgmap, key, |x| x >= a)
}

/// get a list of integers equal or below a certain value
pub fn cfg_vec_int_check_below_eq(cfgmap: &CfgMap, key: &str, a: i64) -> Result<Option<Vec<i64>>> {
    cfg_vec_int_check(cfgmap, key, |x| x <= a)
}

/// get a list of integers provided that they are exactly a certain value
pub fn cfg_vec_int_check_eq(cfgmap: &CfgMap, key: &str, a: i64) -> Result<Option<Vec<i64>>> {
    cfg_vec_int_check(cfgmap, key, |x| x == a)
}

/// get a list of floats
pub fn cfg_vec_float(cfgmap: &CfgMap, key: &str) -> Result<Option<Vec<f64>>> {
    if let Some(item) = cfgmap.get(key) {
        if !item.is_list() {
            return Err(cfg_err_invalid_config(
                key,
                STR_UNKNOWN_VALUE,
                ERR_INVALID_PARAMETER,
            ));
        }
        let mut v: Vec<f64> = Vec::new();
        for elem in item.as_list().unwrap() {
            if !elem.is_float() {
                return Err(cfg_err_invalid_config(
                    key,
                    STR_UNKNOWN_VALUE,
                    ERR_INVALID_PARAMETER_LIST,
                ));
            } else {
                v.push(*elem.as_float().unwrap());
            }
        }
        Ok(Some(v))
    } else {
        Ok(None)
    }
}

/// get a list of floats checking all of them with provided closure
pub fn cfg_vec_float_check<F: Fn(f64) -> bool>(
    cfgmap: &CfgMap,
    key: &str,
    check: F,
) -> Result<Option<Vec<f64>>> {
    if let Some(v) = cfg_vec_float(cfgmap, key)? {
        for elem in v.iter() {
            if !check(*elem) {
                return Err(cfg_err_invalid_config(
                    key,
                    &format!("{elem}"),
                    ERR_INVALID_VALUE_FOR_LIST_ENTRY,
                ));
            }
        }
        Ok(Some(v))
    } else {
        Ok(None)
    }
}

/// get a list of floats in a specific interval (including boundaries `a` and `b`)
pub fn cfg_vec_float_check_interval(
    cfgmap: &CfgMap,
    key: &str,
    a: f64,
    b: f64,
) -> Result<Option<Vec<f64>>> {
    cfg_vec_float_check(cfgmap, key, |x| a <= x && x <= b)
}

/// get a list of floats equal or above a certain value
pub fn cfg_vec_float_check_above_eq(
    cfgmap: &CfgMap,
    key: &str,
    a: f64,
) -> Result<Option<Vec<f64>>> {
    cfg_vec_float_check(cfgmap, key, |x| x >= a)
}

/// get a list of floats equal or below a certain value
pub fn cfg_vec_float_check_below_eq(
    cfgmap: &CfgMap,
    key: &str,
    a: f64,
) -> Result<Option<Vec<f64>>> {
    cfg_vec_float_check(cfgmap, key, |x| x <= a)
}

/// get a list of floats provided that they are exactly a certain value
pub fn cfg_vec_float_check_eq(cfgmap: &CfgMap, key: &str, a: f64) -> Result<Option<Vec<f64>>> {
    cfg_vec_float_check(cfgmap, key, |x| x == a)
}

/// get a list of strings
pub fn cfg_vec_string(cfgmap: &CfgMap, key: &str) -> Result<Option<Vec<String>>> {
    if let Some(item) = cfgmap.get(key) {
        if !item.is_list() {
            return Err(cfg_err_invalid_config(
                key,
                STR_UNKNOWN_VALUE,
                ERR_INVALID_PARAMETER,
            ));
        }
        let mut v: Vec<String> = Vec::new();
        for elem in item.as_list().unwrap() {
            if !elem.is_str() {
                return Err(cfg_err_invalid_config(
                    key,
                    STR_UNKNOWN_VALUE,
                    ERR_INVALID_PARAMETER_LIST,
                ));
            } else {
                v.push(String::from(elem.as_str().unwrap()));
            }
        }
        Ok(Some(v))
    } else {
        Ok(None)
    }
}

/// get a list of strings checking all of them with provided closure
pub fn cfg_vec_string_check<F: Fn(&str) -> bool>(
    cfgmap: &CfgMap,
    key: &str,
    check: F,
) -> Result<Option<Vec<String>>> {
    if let Some(v) = cfg_vec_string(cfgmap, key)? {
        for elem in v.iter() {
            if !check(elem.as_str()) {
                return Err(cfg_err_invalid_config(
                    key,
                    &elem.to_string(),
                    ERR_INVALID_VALUE_FOR_LIST_ENTRY,
                ));
            }
        }
        Ok(Some(v))
    } else {
        Ok(None)
    }
}

/// get a list of strings checking all of them against a fixed string
pub fn cfg_vec_string_check_exact(
    cfgmap: &CfgMap,
    key: &str,
    check: &str,
) -> Result<Option<Vec<String>>> {
    cfg_vec_string_check(cfgmap, key, |s| s == check)
}

/// get a list of strings checking all of them against a fixed string ignoring case
pub fn cfg_vec_string_check_exact_nocase(
    cfgmap: &CfgMap,
    key: &str,
    check: &str,
) -> Result<Option<Vec<String>>> {
    cfg_vec_string_check(cfgmap, key, |s| s.to_uppercase() == check.to_uppercase())
}

/// get a list of strings provided that all of them are in a certain set of strings
pub fn cfg_vec_string_check_within(
    cfgmap: &CfgMap,
    key: &str,
    check: &Vec<&str>,
) -> Result<Option<Vec<String>>> {
    cfg_vec_string_check(cfgmap, key, |x| check.contains(&x))
}

/// get a list of strings provided that all of them are in a certain set of strings ignoring case
pub fn cfg_vec_string_check_within_nocase(
    cfgmap: &CfgMap,
    key: &str,
    check: &Vec<&str>,
) -> Result<Option<Vec<String>>> {
    // TODO: there is probably a less expensive way to do it
    let mut new_check: Vec<String> = Vec::new();
    for x in check {
        new_check.push(x.to_uppercase());
    }
    cfg_vec_string_check(cfgmap, key, |x| new_check.contains(&x.to_uppercase()))
}

/// get a list of strings checking all of them against a regular expression
pub fn cfg_vec_string_check_regex(
    cfgmap: &CfgMap,
    key: &str,
    check: &Regex,
) -> Result<Option<Vec<String>>> {
    cfg_vec_string_check(cfgmap, key, |s| check.is_match(s))
}

// end.
