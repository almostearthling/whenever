// this utility may contain unused functions or other items
#![allow(dead_code)]

/// pub cfghelp
/// 
/// module providing shortcut functions/macros to help configuration of items
/// by providing a CfgMap instance and the key to be retrieved

use cfgmap::CfgMap;
use regex::Regex;

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
            Err(crate::cfghelp::cfg_invalid_config(
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
            Err(crate::cfghelp::cfg_invalid_config(
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
            Err(crate::cfghelp::cfg_invalid_config(
                $key,
                STR_UNKNOWN_VALUE,
                ERR_MISSING_PARAMETER,
            ))
        }
    };
}


// the following is used to build a suitable error to be returned in case the
// provided map and key did not give the expected result
pub fn cfg_invalid_config(key: &str, value: &str, message: &str) -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        format!("{ERR_INVALID_COND_CONFIG}: ({key}={value}) {message}"),
    )
}


/// get a boolean
pub fn cfg_bool(cfgmap: &CfgMap, key: &str) -> std::io::Result<Option<bool>> {
    if let Some(item) = cfgmap.get(key) {
        if !item.is_bool() {
            return Err(cfg_invalid_config(
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
pub fn cfg_int(cfgmap: &CfgMap, key: &str) -> std::io::Result<Option<i64>> {
    if let Some(item) = cfgmap.get(key) {
        if !item.is_int() {
            return Err(cfg_invalid_config(
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
pub fn cfg_int_check<F: FnOnce(i64) -> bool>(cfgmap: &CfgMap, key: &str, check: F) -> std::io::Result<Option<i64>> {
    if let Some(v) = cfg_int(cfgmap, key)? {
        if check(v) {
            Ok(Some(v))
        } else {
            Err(cfg_invalid_config(
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
pub fn cfg_int_check_interval(cfgmap: &CfgMap, key: &str, a: i64, b: i64) -> std::io::Result<Option<i64>> {
    cfg_int_check(cfgmap, key, |x| a <= x && x <= b)
}

/// get an integer equal or above a certain value
pub fn cfg_int_check_above_eq(cfgmap: &CfgMap, key: &str, a: i64) -> std::io::Result<Option<i64>> {
    cfg_int_check(cfgmap, key, |x| x >= a)
}

/// get an integer equal or below a certain value
pub fn cfg_int_check_below_eq(cfgmap: &CfgMap, key: &str, a: i64) -> std::io::Result<Option<i64>> {
    cfg_int_check(cfgmap, key, |x| x <= a)
}

/// get an integer provided that it is exactly a certain value
pub fn cfg_int_check_eq(cfgmap: &CfgMap, key: &str, a: i64) -> std::io::Result<Option<i64>> {
    cfg_int_check(cfgmap, key, |x| x == a)
}


/// get a float without checks
pub fn cfg_float(cfgmap: &CfgMap, key: &str) -> std::io::Result<Option<f64>> {
    if let Some(item) = cfgmap.get(key) {
        if !item.is_float() {
            return Err(cfg_invalid_config(
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
pub fn cfg_float_check<F: FnOnce(f64) -> bool>(cfgmap: &CfgMap, key: &str, check: F) -> std::io::Result<Option<f64>> {
    if let Some(v) = cfg_float(cfgmap, key)? {
        if check(v) {
            Ok(Some(v))
        } else {
            Err(cfg_invalid_config(
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
pub fn cfg_float_check_interval(cfgmap: &CfgMap, key: &str, a: f64, b: f64) -> std::io::Result<Option<f64>> {
    cfg_float_check(cfgmap, key, |x| a <= x && x <= b)
}

/// get a float equal or above a certain value
pub fn cfg_float_check_above_eq(cfgmap: &CfgMap, key: &str, a: f64) -> std::io::Result<Option<f64>> {
    cfg_float_check(cfgmap, key, |x| x >= a)
}

/// get a float equal or below a certain value
pub fn cfg_float_check_below_eq(cfgmap: &CfgMap, key: &str, a: f64) -> std::io::Result<Option<f64>> {
    cfg_float_check(cfgmap, key, |x| x <= a)
}

/// get a float provided that it is exactly a certain value
pub fn cfg_float_check_eq(cfgmap: &CfgMap, key: &str, a: f64) -> std::io::Result<Option<f64>> {
    cfg_float_check(cfgmap, key, |x| x == a)
}



/// get a string without checks
pub fn cfg_string(cfgmap: &CfgMap, key: &str) -> std::io::Result<Option<String>> {
    if let Some(item) = cfgmap.get(key) {
        if !item.is_str() {
            return Err(cfg_invalid_config(
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
pub fn cfg_string_check<F: FnOnce(&str) -> bool>(cfgmap: &CfgMap, key: &str, check: F) -> std::io::Result<Option<String>> {
    if let Some(v) = cfg_string(cfgmap, key)? {
        if check(&v) {
            Ok(Some(v))
        } else {
            Err(cfg_invalid_config(
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
pub fn cfg_string_check_exact(cfgmap: &CfgMap, key: &str, check: &str) -> std::io::Result<Option<String>> {
    cfg_string_check(cfgmap, key, |s| s == check )
}

/// get a string checking it against a fixed string ignoring case
pub fn cfg_string_check_exact_nocase(cfgmap: &CfgMap, key: &str, check: &str) -> std::io::Result<Option<String>> {
    cfg_string_check(cfgmap, key, |s| s.to_uppercase() == check.to_uppercase())
}

/// get a string provided that it is in a certain set of strings
pub fn cfg_string_check_within(cfgmap: &CfgMap, key: &str, check: &Vec<&str>) -> std::io::Result<Option<String>> {
    cfg_string_check(cfgmap, key, |x| check.contains(&x))
}

/// get a string provided that it is in a certain set of strings ignoring case
pub fn cfg_string_check_within_nocase(cfgmap: &CfgMap, key: &str, check: &Vec<&str>) -> std::io::Result<Option<String>> {
    // TODO: there is probably a less expensive way to do it
    let mut new_check: Vec<String> = Vec::new();
    for x in check {
        new_check.push(String::from(x.to_uppercase()));
    }
    cfg_string_check(cfgmap, key, |x| new_check.contains(&String::from(x.to_uppercase())))
}

/// get a string checking it against a regular expression
pub fn cfg_string_check_regex(cfgmap: &CfgMap, key: &str, check: &Regex) -> std::io::Result<Option<String>> {
    cfg_string_check(cfgmap, key, |s| check.is_match(s))
}



// end.
