use super::udp::Udp;
use mlua::{Error, FromLua, Lua, MultiValue};
use socket2::SockAddr;
use std::net::SocketAddr;

pub(super) fn handle(lua: &Lua, udp: &Udp, args: MultiValue) -> Result<usize, Error> {
    // Parse args
    let address: String = String::from_lua(args[0].clone(), lua)?;
    let port: u16 = u16::from_lua(args[1].clone(), lua)?;

    let socket_addr: SocketAddr = format!("{address}:{port}").parse()?;
    let sock_addr: SockAddr = SockAddr::from(socket_addr);

    // Perform
    let socket = udp.socket.lock().map_err(|err| Error::RuntimeError(err.to_string()))?;
    socket.connect(&sock_addr)?;
    Ok(1)
}

#[cfg(test)]
mod tests {
    use mlua::Lua;

    #[tokio::test]
    async fn setsockname() {
        // Test
        let lua = Lua::new();
        crate::preload(&lua).expect("Expected to preload module");
        let script = r#"
                local socket = require('socket')
                local udp = socket.udp()
                return udp:setpeername('127.0.0.1', 3000)
            "#;
        let (status, err): (Option<u16>, Option<mlua::Value>) =
            lua.load(script).eval().expect("Expected to set peer name");
        assert_eq!(err, None);
        assert_eq!(status, Some(1));
    }
}
