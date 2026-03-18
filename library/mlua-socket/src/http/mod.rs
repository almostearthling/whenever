mod request;

use mlua::{Error, Lua, Table};

#[allow(unused)]
const MODULE_SCRIPT: &str = include_str!("http.lua");

pub fn preload(lua: &Lua) -> Result<(), Error> {
    // Configure module table
    let table = lua.create_table()?;
    table.set("request", lua.create_function(request::handle)?)?;

    // Preload module
    let globals = lua.globals();
    let package: Table = globals.get("package")?;
    let loaded: Table = package.get("loaded")?;
    loaded.set("socket.http", table)?;
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
                return require('socket.http')
            "#,
            )
            .eval()
            .expect("Expected to load 'socket.http' module");
    }
}
