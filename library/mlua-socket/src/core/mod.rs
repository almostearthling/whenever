mod connect;
mod gettime;
mod sleep;
mod tcp;
mod tcp4;
mod tcp6;
mod tcp_bind;
mod tcp_client;
mod tcp_close;
mod tcp_connect;
mod tcp_getfd;
mod tcp_getsockname;
mod tcp_listen;
mod tcp_receive;
mod tcp_send;
mod tcp_server;
mod tcp_setoption;
mod tcp_settimeout;
mod tcp_shutdown;
mod udp;
mod udp4;
mod udp6;
mod udp_close;
mod udp_getfd;
mod udp_receive;
mod udp_receivefrom;
mod udp_sendto;
mod udp_setoption;
mod udp_setpeername;
mod udp_setsockname;
mod udp_settimeout;

use super::dns;
use super::headers;


#[cfg(feature = "server")]
mod bind;

use mlua::{Error, Lua, Table, Value};

pub fn preload(lua: &Lua) -> Result<(), Error> {
    // Configure module table submodules
    let table = lua.create_table()?;

    #[cfg(feature = "inline_lua")]
    {
        table.set("url", lua.load("return require('socket.url')").eval::<Table>()?)?;
    }

    // Configure module table direct functions
    table.set("connect", lua.create_function(connect::handle)?)?;
    table.set("gettime", lua.create_function(gettime::handle)?)?;
    table.set("sleep", lua.create_function(sleep::handle)?)?;
    table.set("tcp", lua.create_function(tcp4::handle)?)?;
    table.set("tcp4", lua.create_function(tcp4::handle)?)?;
    table.set("tcp6", lua.create_function(tcp6::handle)?)?;
    table.set("udp", lua.create_function(udp4::handle)?)?;
    table.set("udp4", lua.create_function(udp4::handle)?)?;
    table.set("udp6", lua.create_function(udp6::handle)?)?;
    
    // table.set("dns", lua.load("return require('socket.dns')").eval::<Table>()?)?;
    // table.set("headers", lua.load("return require('socket.headers')").eval::<Table>()?)?;
    dns::preload(lua)?;
    headers::preload(lua)?;

    
    #[cfg(feature = "server")]
    table.set("bind", lua.create_function(bind::handle)?)?;

    // Configure module table direct values
    table.set("_SOCKETINVALID", Value::Number(-1.))?;

    // Configure module metatable
    let metatable = lua.create_table()?;
    table.set_metatable(Some(metatable))?;

    // Preload module
    let globals = lua.globals();
    let package: Table = globals.get("package")?;
    let loaded: Table = package.get("loaded")?;
    loaded.set("socket.core", table)?;

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
            .load("return require('socket.core')")
            .eval()
            .expect("Expected a `core` submodule");
        assert!(module.contains_key("bind").expect("Expected a bind fn"));
    }
}
