use super::tcp::Tcp;
use mlua::{Error, FromLua, Lua, MultiValue};

pub(super) fn handle(lua: &Lua, tcp: &Tcp, args: MultiValue) -> Result<(), Error> {
    let option: String = String::from_lua(args[0].clone(), lua)?;
    match option.as_str() {
        "keepalive" => {
            let value = bool::from_lua(args[1].clone(), lua)?;
            tcp.socket
                .set_keepalive(value)
                .map_err(|err| Error::RuntimeError(err.to_string()))?;
        }
        "linger" => {
            // TODO
        }
        "reuseaddr" => {
            let value = bool::from_lua(args[1].clone(), lua)?;
            tcp.socket
                .set_reuse_address(value)
                .map_err(|err| Error::RuntimeError(err.to_string()))?;
        }
        "tcp-nodelay" => {
            let value = bool::from_lua(args[1].clone(), lua)?;
            tcp.socket
                .set_tcp_nodelay(value)
                .map_err(|err| Error::RuntimeError(err.to_string()))?;
        }
        _ => {}
    };
    Ok(())
}

#[cfg(test)]
mod tests {
    use mlua::Lua;

    #[test]
    fn keepalive() {
        let lua = Lua::new();
        crate::preload(&lua).expect("Expected to preload module");
        let (retval, err): (Option<i32>, Option<String>) = lua
            .load(
                r#"
                local socket = require('socket')
                local master = socket.tcp()
                return master:setoption('keepalive', true)
            "#,
            )
            .eval()
            .expect("Expected to set keepalive option");
        assert_eq!(retval, Some(1));
        assert_eq!(err, None);
    }
}
