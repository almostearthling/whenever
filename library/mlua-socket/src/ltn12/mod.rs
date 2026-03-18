#![cfg(feature = "inline_lua")]

use mlua::{Error, Lua};

const MODULE_SCRIPT: &str = include_str!("ltn12.lua");

pub fn preload(lua: &Lua) -> Result<(), Error> {
    let script = format!("package.preload['ltn12'] = function() {MODULE_SCRIPT} end");
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
                return require('ltn12')
            "#,
            )
            .eval()
            .expect("Expected to load ltn12 module");
    }
}
