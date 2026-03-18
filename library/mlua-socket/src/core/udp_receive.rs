use super::udp::Udp;
use mlua::{Error, Lua, MultiValue};

pub(super) fn handle(lua: &Lua, udp: &Udp, args: MultiValue) -> Result<Vec<u8>, Error> {
    let (datagram, _ip, _port) = super::udp_receivefrom::handle(lua, udp, args)?;
    Ok(datagram)
}
