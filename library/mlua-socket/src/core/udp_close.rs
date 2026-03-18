use super::udp::Udp;
use mlua::{Error, Lua};
use std::net::Shutdown;

pub(super) fn handle(_lua: &Lua, udp: &Udp) -> Result<(), Error> {
    let socket = udp.socket.lock().map_err(|err| Error::RuntimeError(err.to_string()))?;
    let _ = socket.shutdown(Shutdown::Both);
    Ok(())
}
