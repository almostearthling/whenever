use mlua::{MultiValue, UserData, UserDataFields, UserDataMethods, Value};
use socket2::Socket;
use std::sync::{Arc, Mutex};

pub(crate) struct Udp {
    pub(crate) _connected: Mutex<bool>,
    pub(crate) socket: Arc<Mutex<Socket>>,
}

impl UserData for Udp {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("close", |lua, udp, ()| super::udp_close::handle(lua, udp));
        methods.add_method("getfd", |lua, udp, args| {
            match super::udp_getfd::handle(lua, udp, args) {
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
        methods.add_method("receive", |lua, udp, args| {
            match super::udp_receive::handle(lua, udp, args) {
                Ok(datagram) => {
                    let mut retval = MultiValue::new();
                    retval.push_front(Value::String(lua.create_string(datagram.as_slice())?));
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
        methods.add_method("receivefrom", |lua, udp, args| {
            match super::udp_receivefrom::handle(lua, udp, args) {
                Ok((datagram, ip, port)) => {
                    let mut retval = MultiValue::new();
                    retval.push_front(Value::Number((port).into()));
                    retval.push_front(Value::String(lua.create_string(ip)?));
                    retval.push_front(Value::String(lua.create_string(datagram.as_slice())?));
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
        methods.add_method("sendto", |lua, udp, args| {
            match super::udp_sendto::handle(lua, udp, args) {
                Ok(status) => {
                    let mut retval = MultiValue::new();
                    retval.push_front(Value::Number((status as u16).into()));
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
        methods.add_method("setoption", |lua, udp, args| {
            match super::udp_setoption::handle(lua, udp, args) {
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
        methods.add_method("setpeername", |lua, udp, args| {
            match super::udp_setpeername::handle(lua, udp, args) {
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
        methods.add_method("setsockname", |lua, udp, args| {
            match super::udp_setsockname::handle(lua, udp, args) {
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
        methods.add_method("settimeout", |lua, udp, args| {
            match super::udp_settimeout::handle(lua, udp, args) {
                Ok(_) => {}
                Err(_e) => {}
            }
            Ok(mlua::Nil)
        });
    }
    fn add_fields<F: UserDataFields<Self>>(_fields: &mut F) {}
}
