use mlua::{Error, Lua};

pub(super) fn handle(_lua: &Lua, _arg: mlua::Value) -> Result<String, Error> {
    Ok(match hostname::get()?.to_str() {
        Some(hostname) => hostname.to_string(),
        None => "".to_string(),
    })
}

#[cfg(test)]
mod tests {
    use mlua::Lua;

    #[test]
    fn gethostname() {
        let lua = Lua::new();
        crate::preload(&lua).expect("Expected to preload module");
        let hostname: String = lua
            .load(
                r#"
                local dns = require('socket.dns')
                return dns.gethostname()
            "#,
            )
            .eval()
            .expect("Expected to return hostname");
        assert_ne!(hostname, "");
        eprintln!("socket.dns gethostname()={hostname}");
    }
}
