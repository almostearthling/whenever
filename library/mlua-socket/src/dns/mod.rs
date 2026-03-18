mod getaddrinfo;
mod gethostname;

use mlua::{Error, Lua, Table};

pub fn preload(lua: &Lua) -> Result<(), Error> {
    // Configure module table
    let module = lua.create_table()?;
    module.set("getaddrinfo", lua.create_function(getaddrinfo::handle)?)?;
    module.set("gethostname", lua.create_function(gethostname::handle)?)?;

    // Preload module
    let globals = lua.globals();
    let package: Table = globals.get("package")?;
    let loaded: Table = package.get("loaded")?;
    loaded.set("socket.dns", module)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use mlua::{Lua, Table};

    #[test]
    fn preload() {
        let lua = Lua::new();
        super::preload(&lua).expect("Expected to preload module");
        let _module: Table = lua
            .load("return require('socket.dns')")
            .eval()
            .expect("Expected to load 'socket.dns' module");
    }
}
