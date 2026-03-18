use super::tcp::Tcp;
use mlua::{Error, Lua, MultiValue};

pub(super) fn handle(_lua: &Lua, tcp: &Tcp, _args: MultiValue) -> Result<(String, u16, String), Error> {
    let local_addr = tcp
        .socket
        .local_addr()
        .map_err(|err| Error::RuntimeError(err.to_string()))?;
    let (addr, port) = match local_addr.as_socket() {
        Some(addr) => (addr.ip().to_string(), addr.port()),
        None => return Err(Error::RuntimeError("Cannot determine address".to_string())),
    };
    let family = {
        if local_addr.is_ipv4() {
            "inet".to_string()
        } else if local_addr.is_ipv6() {
            "inet6".to_string()
        } else {
            "".to_string()
        }
    };
    Ok((addr, port, family))
}

#[cfg(test)]
mod tests {
    use mlua::Lua;

    #[tokio::test]
    async fn getsockname() {
        // Setup a listener
        let lua = Lua::new();
        crate::preload(&lua).expect("Expected to preload module");
        let script = r#"
                local socket = require('socket')
                local server = assert(socket.bind('127.0.0.1', 0))
                return server:getsockname()
            "#;
        let (ip_addr, port, family): (String, i32, String) =
            lua.load(script).eval().expect("Expected to access socket name");
        assert_eq!(ip_addr, "127.0.0.1");
        assert!(port > 0);
        assert_eq!(family, "inet");
    }
}
