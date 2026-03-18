#![cfg(feature = "inline_lua")]

use mlua::{Error, Lua};

const MODULE_SCRIPT: &str = include_str!("url.lua");

pub fn preload(lua: &Lua) -> Result<(), Error> {
    let script = format!("package.preload['socket.url'] = function() {MODULE_SCRIPT} end");
    lua.load(script).exec()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use mlua::Lua;

    #[test]
    fn preload() {
        let lua = Lua::new();
        crate::preload(&lua).expect("Expected to preload module");
        let (scheme, path): (String, String) = lua
            .load(
                r#"
                local url = require('socket.url')
                local parsed_url = url.parse('http://www.example.com/cgilua/index.lua')
                return parsed_url.scheme, parsed_url.path 
            "#,
            )
            .eval()
            .expect("Expected to parse an http url");
        assert_eq!(scheme, "http");
        assert_eq!(path, "/cgilua/index.lua");
    }
}
