#![cfg(feature = "inline_lua")]

use mlua::{Error, Lua};

const MODULE_SCRIPT: &str = include_str!("except.lua");

pub fn preload(lua: &Lua) -> Result<(), Error> {
    let script = format!("package.preload['socket.except'] = function() {MODULE_SCRIPT} end");
    lua.load(script).exec()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use mlua::{Lua, Table};

    #[test]
    fn preload() {
        let lua = Lua::new();
        crate::preload(&lua).expect("Expected to preload module");
        let _module: Table = lua
            .load(
                r#"
                return require('socket.except')
            "#,
            )
            .eval()
            .expect("Expected to load 'socket.except' module");
    }

    #[test]
    fn excepttest() {
        static EXCEPTTEST_SCRIPT: &str = include_str!("excepttest.lua");
        let lua = Lua::new();
        crate::preload(&lua).expect("Expected to preload module");
        lua.load(EXCEPTTEST_SCRIPT)
            .exec()
            .expect("Expected to run excepttest.lua script");
    }
}
