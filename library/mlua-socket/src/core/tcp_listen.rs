use super::tcp::Tcp;
use mlua::{Error, FromLua, Lua, MultiValue};

pub(super) fn handle(lua: &Lua, tcp: &Tcp, args: MultiValue) -> Result<(), Error> {
    // Parse args
    let backlog: i32 = {
        if !args.is_empty() {
            i32::from_lua(args[0].clone(), lua).map_err(|err| Error::RuntimeError(err.to_string()))?
        } else {
            32
        }
    };

    // Listen
    tcp.socket
        .listen(backlog)
        .map_err(|err| Error::RuntimeError(err.to_string()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use mlua::Lua;

    #[tokio::test]
    async fn listen() {
        // Setup a listener
        let lua = Lua::new();
        crate::preload(&lua).expect("Expected to preload module");
        let script = r#"
                local socket = require('socket')
                local server = assert(socket.bind('127.0.0.1', 0))
                return server:listen()
            "#;
        let (retval, err): (Option<u16>, Option<String>) = lua
            .load(script)
            .eval()
            .expect("Expected to listen on the loopback adapter with a dynamic port assignment");
        assert_eq!(retval, Some(1));
        assert_eq!(err, None);
    }
}
