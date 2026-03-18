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

#[cfg(test)]
mod tests {
    use mlua::Lua;
    use std::net::TcpListener;

    #[test]
    fn connect() {
        // Setup a listener to connect to
        let socket =
            TcpListener::bind("127.0.0.1:0").expect("Expected to bind to loopback addr with a dynamic port assignment");
        let local_addr = socket.local_addr().expect("Expected to obtain local address");

        // Test
        let lua = Lua::new();
        crate::preload(&lua).expect("Expected to preload module");
        let script = r#"
                local socket = require('socket')
                local master = socket.tcp()
                return master:connect('127.0.0.1', _port_)
            "#
        .replace("_port_", format!("{}", local_addr.port()).as_str());
        let (ok, err): (bool, Option<String>) = lua.load(script).eval().expect("Expected to connect to local listener");
        assert_eq!(ok, true);
        assert_eq!(err, None);
    }

    #[test]
    fn connect_to_bad_port() {
        // Test
        let lua = Lua::new();
        crate::preload(&lua).expect("Expected to preload module");
        let script = r#"
                local socket = require('socket')
                local master = socket.tcp()
                return master:connect('127.0.0.1', 1234)
            "#;
        let (ok, err): (bool, Option<String>) = lua
            .load(script)
            .eval()
            .expect("Expected to attempt connection to bad port");
        assert_eq!(ok, false);
        assert_ne!(err, None);
    }
}
