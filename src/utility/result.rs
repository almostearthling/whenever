//! A common result type: catching errors from modules used throughout
//! the entire code. The corresponding error carries some information
//! about what went wrong.

use mlua;
use notify;
use std::{self, fmt, sync::PoisonError};

use crate::constants::{ERR_FAILED, ERR_LOCK_FAILED};

/// Types of specific errors
#[non_exhaustive]
#[derive(Debug, Clone)]
pub enum Kind {
    Forbidden,
    Unsupported,
    Unavailable,
    Unconverted,
    Unparsed,
    Busy,
    Invalid,
    Failed,
    Empty,
    // ...
    Unknown,
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Kind::Forbidden => "not permitted",
                Kind::Unsupported => "not supported",
                Kind::Unavailable => "not available",
                Kind::Unconverted => "not converted",
                Kind::Unparsed => "not parsed",
                Kind::Busy => "resource busy",
                Kind::Invalid => "invalid",
                Kind::Failed => "failed",
                Kind::Empty => "empty",
                Kind::Unknown => "unknown",
            }
        )
    }
}

/// Describes the origin of the error: if `Native` the error was originated
/// natively, otherwise the field is set by another error that is converted
/// into `Error` via a dedicated `From` trait implementation.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum Origin {
    Native,
    Unit,
    StdIo,
    Notify,
    Sync,
    Lua,

    #[cfg(feature = "dbus")]
    DBus,

    #[cfg(windows)]
    #[cfg(feature = "wmi")]
    Wmi,

    // ...
    Unknown,
}

impl fmt::Display for Origin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Origin::Native => "self",
                Origin::Unit => "unit",
                Origin::StdIo => "io",
                Origin::Notify => "fschange",
                Origin::Sync => "sync",
                Origin::Lua => "lua",

                #[cfg(feature = "dbus")]
                Origin::DBus => "dbus",

                #[cfg(windows)]
                #[cfg(feature = "wmi")]
                Origin::Wmi => "wmi",

                // ...
                Origin::Unknown => "unknown",
            }
        )
    }
}

/// The error type that is used throughout the application: implementations
/// of the `From` trait are used to implicitly convert from other error
/// types, which in turn set the `origin` property.
#[derive(Debug, Clone)]
pub struct Error {
    kind: Kind,
    origin: Origin,
    message: String, // freeform message: owned in order to avoid lifetime management
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.origin != Origin::Native {
            write!(f, "{} ({}): {}", self.kind, self.origin, self.message)
        } else {
            write!(f, "{}: {}", self.kind, self.message)
        }
    }
}

// maybe the most important: From<std::io::Error>
impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self {
            kind: match e.kind() {
                std::io::ErrorKind::Unsupported => Kind::Unsupported,
                std::io::ErrorKind::PermissionDenied => Kind::Forbidden,
                std::io::ErrorKind::InvalidData => Kind::Invalid,
                std::io::ErrorKind::InvalidInput => Kind::Invalid,
                _ => Kind::Unknown,
            },
            origin: Origin::StdIo,
            message: e.to_string(),
        }
    }
}

// notify (fschange) errors
impl From<notify::Error> for Error {
    fn from(e: notify::Error) -> Self {
        Self {
            kind: Kind::Failed,
            origin: Origin::Notify,
            message: e.to_string(),
        }
    }
}

// Lua errors
impl From<mlua::Error> for Error {
    fn from(e: mlua::Error) -> Self {
        Self {
            kind: Kind::Failed,
            origin: Origin::Lua,
            message: e.to_string(),
        }
    }
}

// zbus errors
#[cfg(feature = "dbus")]
impl From<zbus::Error> for Error {
    fn from(e: zbus::Error) -> Self {
        Self {
            kind: Kind::Failed,
            origin: Origin::DBus,
            message: e.to_string(),
        }
    }
}

// wmi errors
#[cfg(windows)]
#[cfg(feature = "wmi")]
impl From<wmi::WMIError> for Error {
    fn from(e: wmi::WMIError) -> Self {
        let kind = match e {
            wmi::WMIError::ConvertBoolError(_)
            | wmi::WMIError::ConvertStringError(_)
            | wmi::WMIError::ConvertLengthError(_)
            | wmi::WMIError::ConvertDatetimeError(_)
            | wmi::WMIError::ConvertDurationError(_)
            | wmi::WMIError::ConvertVariantError(_)
            | wmi::WMIError::ConvertError(_) => Kind::Unconverted,
            wmi::WMIError::DeserializeValueError(_)
            | wmi::WMIError::InvalidDeserializationVariantError(_)
            | wmi::WMIError::SerdeError(_) => Kind::Invalid,
            wmi::WMIError::ParseDatetimeError(_)
            | wmi::WMIError::ParseFloatError(_)
            | wmi::WMIError::ParseIntError(_) => Kind::Unparsed,
            wmi::WMIError::UnimplementedArrayItem => Kind::Unavailable,
            _ => Kind::Failed,
        };
        Self {
            kind,
            origin: Origin::Wmi,
            message: e.to_string(),
        }
    }
}

// resource locking errors
impl<T> From<PoisonError<T>> for Error {
    fn from(_: PoisonError<T>) -> Self {
        Self {
            kind: Kind::Failed,
            origin: Origin::Sync,
            message: ERR_LOCK_FAILED.to_owned(),
        }
    }
}

// errors based on the unit type
impl From<()> for Error {
    fn from(_: ()) -> Self {
        Self {
            kind: Kind::Failed,
            origin: Origin::Unit,
            message: ERR_FAILED.to_owned(),
        }
    }
}

// implements `Error` and provides access to properties
impl Error {
    // this is used only to natively create an instance of `Error`: only
    // conversions set the `origin` property to something different
    pub fn new(kind: Kind, message: &str) -> Self {
        Self {
            kind,
            origin: Origin::Native,
            message: message.to_string(),
        }
    }

    // property access
    pub fn kind(&self) -> &Kind {
        &self.kind
    }

    pub fn origin(&self) -> &Origin {
        &self.origin
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

// possible last resort to allow conversions from pointers to errors
impl<T: std::error::Error> From<Box<T>> for Error {
    fn from(e: Box<T>) -> Self {
        Self {
            kind: Kind::Unknown,
            origin: Origin::Unknown,
            message: e.to_string(),
        }
    }
}

/// Specific `Result` type that assumes `wres::Error` as its Err variant
pub type Result<T> = std::result::Result<T, Error>;
