use mlua::Error::RuntimeError;
use mlua::{Error, Lua};
use std::time::{SystemTime, UNIX_EPOCH};

pub(super) fn handle(_lua: &Lua, _arg: mlua::Value) -> Result<mlua::Number, Error> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|err| RuntimeError(err.to_string()))?;
    Ok(now.as_nanos() as f64 / 1e9_f64)
}

#[cfg(test)]
mod tests {
    use mlua::Lua;

    #[test]
    fn gettime() {
        let lua = Lua::new();
        crate::preload(&lua).expect("Expected to preload module");
        let retval: f64 = lua
            .load(
                r#"
                local socket = require('socket')
                return socket.gettime()
            "#,
            )
            .eval()
            .expect("Expected to get current time");
        assert!(retval > 1690028126_f64);
    }
}
