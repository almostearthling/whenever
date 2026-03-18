use super::udp::Udp;
use mlua::{Error, FromLua, Lua, MultiValue};

pub(super) fn handle(lua: &Lua, udp: &Udp, args: MultiValue) -> Result<(), Error> {
    let option: String = String::from_lua(args[0].clone(), lua)?;
    let socket = udp.socket.lock().map_err(|err| Error::RuntimeError(err.to_string()))?;
    match option.as_str() {
        "broadcast" => {
            let value = bool::from_lua(args[1].clone(), lua)?;
            socket
                .set_broadcast(value)
                .map_err(|err| Error::RuntimeError(err.to_string()))?;
        }
        "reuseaddr" => {
            let value = bool::from_lua(args[1].clone(), lua)?;
            socket
                .set_reuse_address(value)
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
    fn broadcast() {
        let lua = Lua::new();
        crate::preload(&lua).expect("Expected to preload module");
        let (retval, err): (Option<i32>, Option<String>) = lua
            .load(
                r#"
                local socket = require('socket')
                local udp = socket.udp()
                return udp:setoption('broadcast', true)
            "#,
            )
            .eval()
            .expect("Expected to set broadcast udp option");
        assert_eq!(retval, Some(1));
        assert_eq!(err, None);
    }

    #[test]
    fn reuseaddr() {
        let lua = Lua::new();
        crate::preload(&lua).expect("Expected to preload module");
        let (retval, err): (Option<i32>, Option<String>) = lua
            .load(
                r#"
                local socket = require('socket')
                local udp = socket.udp()
                return udp:setoption('reuseaddr', true)
            "#,
            )
            .eval()
            .expect("Expected to set reuseaddr udp option");
        assert_eq!(retval, Some(1));
        assert_eq!(err, None);
    }
}
