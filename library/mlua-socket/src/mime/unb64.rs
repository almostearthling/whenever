use base64::{Engine as _, engine::general_purpose};
use mlua::{Error, Lua};

pub(super) fn handle(_lua: &Lua, encoded: mlua::String) -> Result<String, Error> {
    let encoded_as_str = encoded.to_str().map_err(|err| Error::RuntimeError(err.to_string()))?;
    let decoded: Vec<u8> = general_purpose::STANDARD
        .decode(encoded_as_str.as_bytes())
        .map_err(|err| Error::RuntimeError(err.to_string()))?;
    let decoded_as_str = std::str::from_utf8(decoded.as_slice()).map_err(|err| Error::RuntimeError(err.to_string()))?;
    Ok(decoded_as_str.to_string())
}

#[cfg(test)]
mod tests {
    use mlua::Lua;

    #[test]
    fn unb64() {
        let lua = Lua::new();
        crate::preload(&lua).expect("Expected to preload module");
        let encoded: String = lua
            .load(
                r#"
                local mime = require('mime')
                return mime.unb64('YWJjZA==')
            "#,
            )
            .eval()
            .expect("Expected to base64-decode a few chars");
        assert_eq!(encoded, "abcd");
    }
}
