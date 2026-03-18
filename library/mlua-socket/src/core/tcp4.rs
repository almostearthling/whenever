use super::tcp::Tcp;
use mlua::{Error, Lua, MultiValue};
use socket2::{Domain, Socket, Type};

pub(super) fn handle(_lua: &Lua, _args: MultiValue) -> Result<Tcp, Error> {
    let socket = Socket::new(Domain::IPV4, Type::STREAM, None)?;
    Ok(Tcp { socket })
}
