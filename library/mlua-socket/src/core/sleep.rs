use mlua::{Error, FromLua, Lua};
use std::thread;
use std::time::Duration;

pub(super) fn handle(lua: &Lua, arg: mlua::Value) -> Result<(), Error> {
    let dur = Duration::from_secs_f64(f64::from_lua(arg, lua)?);
    thread::sleep(dur);
    Ok(())
}

#[cfg(test)]
mod tests {
    use mlua::Lua;
    use std::time::Instant;

    #[test]
    fn sleep() {
        let lua = Lua::new();
        crate::preload(&lua).expect("Expected to preload module");
        let start_time = Instant::now();
        lua.load(
            r#"
                local socket = require('socket')
                socket.sleep(0.2)
            "#,
        )
        .exec()
        .expect("Expected to sleep");
        let elapsed = start_time.elapsed();
        assert!(elapsed.as_millis() > 190);
        assert!(elapsed.as_millis() < 400);
    }
}
