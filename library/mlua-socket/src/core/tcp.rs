use mlua::{MultiValue, UserData, UserDataFields, UserDataMethods, Value};
use socket2::Socket;

pub(crate) struct Tcp {
    pub(crate) socket: Socket,
}

impl UserData for Tcp {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        #[cfg(feature = "server")]
        methods.add_method("bind", |lua, tcp, args| {
            match super::tcp_bind::handle(lua, tcp, args) {
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
        methods.add_method("close", |lua, tcp, ()| super::tcp_close::handle(lua, tcp));
        methods.add_method("connect", |lua, tcp, args| {
            match super::tcp_connect::handle(lua, tcp, args) {
                Ok(_) => {
                    let mut retval = MultiValue::new();
                    retval.push_front(mlua::Nil); // err
                    retval.push_front(Value::Boolean(true)); // ok
                    Ok(retval)
                }
                Err(e) => {
                    let mut retval = MultiValue::new();
                    retval.push_front(Value::String(lua.create_string(e.to_string())?)); // err
                    retval.push_front(Value::Boolean(false)); // ok
                    Ok(retval)
                }
            }
        });
        methods.add_method("getfd", |lua, tcp, args| {
            match super::tcp_getfd::handle(lua, tcp, args) {
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
        methods.add_method("getsockname", |lua, tcp, args| {
            match super::tcp_getsockname::handle(lua, tcp, args) {
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
            }
        });
        methods.add_method("listen", |lua, tcp, args| {
            match super::tcp_listen::handle(lua, tcp, args) {
                Ok(()) => {
                    let mut retval = MultiValue::new();
                    retval.push_front(Value::Number(1.));
                    Ok(retval)
                }
                Err(e) => {
                    let mut retval = MultiValue::new();
                    retval.push_front(Value::String(lua.create_string(e.to_string())?));
                    retval.push_front(mlua::Nil);
                    Ok(retval)
                }
            }
        });
        methods.add_method("receive", |lua, tcp, args| {
            match super::tcp_receive::handle(lua, tcp, args) {
                Ok(data) => {
                    let mut retval = MultiValue::new();
                    retval.push_front(Value::String(lua.create_string(data)?));
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
        methods.add_method("send", |lua, tcp, args| {
            match super::tcp_send::handle(lua, tcp, args) {
                Ok(index) => {
                    let mut retval = MultiValue::new();
                    retval.push_front(Value::Number((index as u16).into()));
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
        methods.add_method("setoption", |lua, tcp, args| {
            match super::tcp_setoption::handle(lua, tcp, args) {
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
        methods.add_method("settimeout", |lua, tcp, args| {
            match super::tcp_settimeout::handle(lua, tcp, args) {
                Ok(_) => {}
                Err(_e) => {}
            }
            Ok(mlua::Nil)
        });
    }
    fn add_fields<F: UserDataFields<Self>>(_fields: &mut F) {}
}
