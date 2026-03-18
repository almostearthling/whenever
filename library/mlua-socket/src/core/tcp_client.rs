use super::tcp::Tcp;
use mlua::{IntoLuaMulti, MultiValue, UserData, UserDataMethods, Value};

pub(crate) struct TcpClient {
    pub(crate) tcp: Tcp,
}

impl UserData for TcpClient {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("close", |lua, tcp_client, ()| {
            super::tcp_close::handle(lua, &tcp_client.tcp)
        });
        methods.add_method("getfd", |lua, tcp_client, args| {
            match super::tcp_getfd::handle(lua, &tcp_client.tcp, args) {
                Ok(fd) => {
                    let mut retval = MultiValue::new();
                    retval.push_front(Value::Number(fd.into()));
                    Ok(retval)
                }
                Err(_e) => {
                    let mut retval = MultiValue::new();
                    retval.push_front(Value::Number(-1.));
                    Ok(retval)
                }
            }
        });
        methods.add_method(
            "getsockname",
            |lua, tcp_client, args| match super::tcp_getsockname::handle(lua, &tcp_client.tcp, args) {
                Ok((addr, port, family)) => {
                    let mut retval = MultiValue::new();
                    retval.push_front(Value::String(lua.create_string(family)?));
                    retval.push_front(Value::Number(port.into()));
                    retval.push_front(Value::String(lua.create_string(addr)?));
                    Ok(retval)
                }
                Err(_e) => {
                    let mut retval = MultiValue::new();
                    retval.push_front(mlua::Nil);
                    Ok(retval)
                }
            },
        );
        methods.add_method("receive", |lua, tcp_server, args| {
            match super::tcp_receive::handle(lua, &tcp_server.tcp, args) {
                Ok(retval) => Ok(retval.into_lua_multi(lua)?),
                Err(e) => {
                    let mut retval = MultiValue::new();
                    retval.push_front(Value::String(lua.create_string(e.to_string())?)); // err
                    retval.push_front(mlua::Nil);
                    Ok(retval)
                }
            }
        });
        methods.add_method("send", |lua, tcp_server, args| {
            match super::tcp_send::handle(lua, &tcp_server.tcp, args) {
                Ok(retval) => Ok(retval.into_lua_multi(lua)?),
                Err(e) => {
                    let mut retval = MultiValue::new();
                    retval.push_front(Value::String(lua.create_string(e.to_string())?)); // err
                    retval.push_front(mlua::Nil);
                    Ok(retval)
                }
            }
        });
        methods.add_method("setoption", |lua, tcp_client, args| {
            match super::tcp_setoption::handle(lua, &tcp_client.tcp, args) {
                Ok(_) => {
                    let mut retval = MultiValue::new();
                    retval.push_front(Value::Number(1.));
                    Ok(retval)
                }
                Err(e) => {
                    let mut retval = MultiValue::new();
                    retval.push_front(Value::String(lua.create_string(e.to_string())?)); // err
                    retval.push_front(mlua::Nil);
                    Ok(retval)
                }
            }
        });
        methods.add_method("settimeout", |lua, tcp_client, args| {
            match super::tcp_settimeout::handle(lua, &tcp_client.tcp, args) {
                Ok(_) => {}
                Err(_e) => {}
            }
            Ok(mlua::Nil)
        });
        methods.add_method("shutdown", |lua, tcp_client, args| {
            match super::tcp_shutdown::handle(lua, &tcp_client.tcp, args) {
                Ok(_) => {}
                Err(_e) => {}
            }
            Ok(Value::Number(1.))
        });
    }
}
