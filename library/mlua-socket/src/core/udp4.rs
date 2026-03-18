use super::udp::Udp;
use mlua::{Error, Lua};
use socket2::{Domain, Socket, Type};
use std::sync::{Arc, Mutex};

pub(super) fn handle(_lua: &Lua, _arg: mlua::Value) -> Result<Udp, Error> {
    let connected = Mutex::new(false);
    let socket = Arc::new(Mutex::new(Socket::new(Domain::IPV4, Type::DGRAM, None)?));
    Ok(Udp {
        _connected: connected,
        socket,
    })
}
