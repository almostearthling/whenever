#![cfg(feature = "server")]

use super::tcp::Tcp;
use mlua::{Error, FromLua, Lua, MultiValue};
use std::net::SocketAddr;

pub(super) fn handle(lua: &Lua, tcp: &Tcp, args: MultiValue) -> Result<(), Error> {
    // Parse args
    let addr: String = String::from_lua(args[0].clone(), lua)?;
    let port: u16 = u16::from_lua(args[1].clone(), lua)?;
    // TODO locaddr args[2]
    // TODO locport args[3]
    // TODO family args[4]
    let socket_addr: SocketAddr = format!("{addr}:{port}").parse()?;

    // Connect
    tcp.socket.connect(&socket_addr.into())?;
    Ok(())
}
