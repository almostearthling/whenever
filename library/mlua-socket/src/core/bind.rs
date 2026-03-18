#![cfg(feature = "server")]

use super::tcp_server::TcpServer;
use mlua::{Error, FromLua, Lua, MultiValue};
use socket2::SockAddr;
use std::net::SocketAddr;

pub(super) fn handle(lua: &Lua, args: MultiValue) -> Result<TcpServer, Error> {
    // Parse args
    let addr = String::from_lua(args[0].clone(), lua)?;
    let port: u16 = u16::from_lua(args[1].clone(), lua)?;
    let sock_addr: SockAddr = {
        let socket_addr: SocketAddr = format!("{addr}:{port}").parse()?;
        socket_addr.into()
    };
    let _backlog = {
        if args.len() >= 3 {
            u16::from_lua(args[2].clone(), lua).map_err(|err| Error::RuntimeError(err.to_string()))?
        } else {
            32_u16
        }
    };

    // Bind
    let tcp = super::tcp4::handle(lua, args)?;
    {
        tcp.socket
            .set_reuse_address(true)
            .map_err(|err| Error::RuntimeError(err.to_string()))?;
        tcp.socket
            .bind(&sock_addr)
            .map_err(|err| Error::RuntimeError(err.to_string()))?;
    }
    Ok(TcpServer { tcp })
}

#[cfg(test)]
mod tests {
    use mlua::Lua;

    #[test]
    fn bind() {
        let lua = Lua::new();
        crate::preload(&lua).expect("Expected to preload module");
        lua.load(
            r#"
                local socket = require('socket.core')
                local server = socket.bind('127.0.0.1', 0)
            "#,
        )
        .exec()
        .expect("Expected to bind to loopback addr");
    }
}
