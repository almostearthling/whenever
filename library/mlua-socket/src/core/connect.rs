use super::tcp_client::TcpClient;
use mlua::{Error, Lua, MultiValue};

pub(super) fn handle(lua: &Lua, args: MultiValue) -> Result<TcpClient, Error> {
    let tcp = super::tcp4::handle(lua, args.clone())?;
    super::tcp_connect::handle(lua, &tcp, args).map_err(|err| Error::RuntimeError(err.to_string()))?;
    Ok(TcpClient { tcp })
}

#[cfg(test)]
mod tests {
    use mlua::Lua;
    use std::net::TcpListener;

    #[test]
    fn connect() {
        // Setup a listener to connect to
        let socket = TcpListener::bind("127.0.0.1:0").expect("Expected to bind to loopback addr on a dynamic port");
        let local_addr = socket.local_addr().expect("Expected to access addr of dynamic port");

        // Test
        let lua = Lua::new();
        crate::preload(&lua).expect("Expected to preload module");
        let script = r#"
                local socket = require('socket')
                local client = socket.connect('127.0.0.1', _port_)
                return client:getsockname()
            "#
        .replace("_port_", format!("{}", local_addr.port()).as_str());
        let (ip_addr, port, family): (String, i32, String) = lua
            .load(script)
            .eval()
            .expect("Expected to access socket name and other metadata");
        assert_eq!(ip_addr, "127.0.0.1");
        assert!(port > 0);
        assert_eq!(family, "inet");
    }
}
