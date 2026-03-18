mod core;
mod mime;
mod dns;
mod headers;
mod http;

#[cfg(feature = "inline_lua")]
mod ltn12;
#[cfg(feature = "inline_lua")]
mod url;
#[cfg(feature = "inline_lua")]
mod except;


use mlua::{Error, Lua};

pub fn preload(lua: &Lua) -> Result<(), Error> {
    // Preload modules
    dns::preload(lua)?;
    headers::preload(lua)?;
    core::preload(lua)?;
    mime::preload(lua)?;
    http::preload(lua)?;
    
    #[cfg(feature = "inline_lua")]
    {
        
        ltn12::preload(lua)?;
        url::preload(lua)?;
        except::preload(lua)?;
        
        const MODULE_SCRIPT: &str = include_str!("socket.lua");
        let script = format!("package.preload['socket'] = function() {MODULE_SCRIPT} end");
        lua.load(script).exec()?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use mlua::{Lua, Table};

    #[test]
    fn load() {
        let lua = Lua::new();
        crate::preload(&lua).expect("Expected to preload module");
        let module: Table = lua
            .load("return require('socket')")
            .eval()
            .expect("Expected to load socket module");
        // assert!(module.contains_key("core").expect("Expected a 'core' submodule"));
        assert!(module.contains_key("dns").expect("Expected a 'dns' submodule"));
        // assert!(module.contains_key("http").expect("Expected a 'http' submodule"));
        assert!(module.contains_key("headers").expect("Expected a 'headers' submodule"));
        assert!(module.contains_key("url").expect("Expected a 'url' submodule"));
    }
}
