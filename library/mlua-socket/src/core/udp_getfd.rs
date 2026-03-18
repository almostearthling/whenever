use super::udp::Udp;
use mlua::{Error, Lua, MultiValue};

#[cfg(target_family = "unix")]
pub(super) fn handle(_lua: &Lua, udp: &Udp, _args: MultiValue) -> Result<i32, Error> {
    use std::os::fd::AsRawFd;
    let socket = udp.socket.lock().map_err(|err| Error::RuntimeError(err.to_string()))?;
    let fd = socket.as_raw_fd();
    Ok(fd)
}

#[cfg(target_family = "windows")]
pub(super) fn handle(_lua: &Lua, _udp: &Udp, _args: MultiValue) -> Result<i32, Error> {
    Err(Error::RuntimeError("Unavailable on this platform".to_string()))
}
