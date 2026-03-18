use mlua::{Error, Lua, Table};

mod b64;
mod unb64;

pub fn preload(lua: &Lua) -> Result<(), Error> {
    // Configure module table
    let module = lua.create_table()?;
    module.set("b64", lua.create_function(b64::handle)?)?;
    module.set("unb64", lua.create_function(unb64::handle)?)?;

    // Preload module
    let globals = lua.globals();
    let package: Table = globals.get("package")?;
    let loaded: Table = package.get("loaded")?;
    loaded.set("mime", module)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use mlua::{Lua, Table};
    use std::error::Error;

    #[test]
    fn preload() -> Result<(), Box<dyn Error>> {
        let lua = Lua::new();
        super::preload(&lua)?;
        let module: Table = lua.load("return require('mime')").eval()?;
        assert!(module.contains_key("b64")?);
        assert!(module.contains_key("unb64")?);
        Ok(())
    }
}
