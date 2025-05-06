//! event module: defines all types of event

pub mod base; // this only defines the trait
pub mod registry; // the main event registry

// specific event types
pub mod fschange_event;
pub mod manual_event;

#[cfg(feature = "dbus")]
pub mod dbus_event;

#[cfg(windows)]
#[cfg(feature = "wmi")]
pub mod wmi_event;

// end.
