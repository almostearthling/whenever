use base64::{Engine as _, engine::general_purpose};
use mlua::{Error, Lua};

pub(super) fn handle(_lua: &Lua, data: mlua::String) -> Result<String, Error> {
    let encoded: String = general_purpose::STANDARD.encode(data.as_bytes());
    Ok(encoded)
}

#[cfg(test)]
mod tests {
    use mlua::Lua;

    #[test]
    fn b64() {
        let lua = Lua::new();
        crate::preload(&lua).expect("Expected to preload module");
        let encoded: String = lua
            .load(
                r#"
                local mime = require('mime')
                return mime.b64('abcd')
            "#,
            )
            .eval()
            .expect("Expected to base64-encode a few chars");
        assert_eq!(encoded, "YWJjZA==");
    }
}
