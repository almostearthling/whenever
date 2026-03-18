use super::udp::Udp;
use mlua::{Error, FromLua, Lua, MultiValue};
use std::time::Duration;

pub(super) fn handle(lua: &Lua, udp: &Udp, args: MultiValue) -> Result<(), Error> {
    // Parse arg
    let timeout = f32::from_lua(args[0].clone(), lua).map_err(|err| Error::RuntimeError(err.to_string()))?;

    // Perform
    let duration = Duration::from_millis((timeout * 1e6) as u64);
    let socket = udp.socket.lock().map_err(|err| Error::RuntimeError(err.to_string()))?;
    socket.set_read_timeout(Some(duration))?;
    socket.set_write_timeout(Some(duration))?;

    Ok(())
}
