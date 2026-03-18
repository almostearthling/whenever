use super::tcp::Tcp;
use mlua::{Error, Lua, MultiValue};

#[cfg(target_family = "unix")]
pub(super) fn handle(_lua: &Lua, tcp: &Tcp, _args: MultiValue) -> Result<i32, Error> {
    use std::os::fd::AsRawFd;
    let fd = tcp.socket.as_raw_fd();
    Ok(fd)
}

#[cfg(target_family = "windows")]
pub(super) fn handle(_lua: &Lua, _tcp: &Tcp, _args: MultiValue) -> Result<i32, Error> {
    Err(Error::RuntimeError("Unavailable on this platform".to_string()))
}

#[cfg(target_family = "unix")]
#[cfg(test)]
mod tests {
    use mlua::Lua;

    #[tokio::test]
    #[cfg(target_family = "unix")]
    async fn getfd_server() {
        // Setup a listener
        let lua = Lua::new();
        crate::preload(&lua).expect("Expected to preload module");
        let script = r#"
                local socket = require('socket')
                local server = assert(socket.bind('127.0.0.1', 0))
                return server:getfd()
            "#;
        let fd: i32 = lua
            .load(script)
            .eval()
            .expect("Expected to access socket file descriptor");
        assert!(fd > 0);
    }
}
