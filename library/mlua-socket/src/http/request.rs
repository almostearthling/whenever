use mlua::{Error, FromLua, Lua, MultiValue, Table, Value};
use reqwest::Method;
use reqwest::blocking::Client;
use reqwest::header::{HeaderMap, HeaderName};
use std::collections::HashMap;

pub(super) fn handle(lua: &Lua, args: MultiValue) -> Result<(Option<String>, u16), Error> {
    // Parse args
    if let Ok(url) = String::from_lua(args[0].clone(), lua) {
        let method = "GET";
        let headers = None;
        handle_request(&url, method, headers)
    } else if let Ok(table) = Table::from_lua(args[0].clone(), lua) {
        let url: String = match table.get("url") {
            Ok(url) => url,
            Err(e) => return Err(Error::RuntimeError(e.to_string())),
        };
        let method = {
            let method: Value = table.get("method").map_err(|e| Error::RuntimeError(e.to_string()))?;
            if method == Value::Nil {
                "GET".to_string()
            } else {
                method.to_string().map_err(|e| Error::RuntimeError(e.to_string()))?
            }
        };
        let headers = {
            match table
                .get::<Table>("headers")
                .map_err(|e| Error::RuntimeError(e.to_string()))
            {
                Ok(headers_table) => {
                    let hdrs: Vec<mlua::Result<(String, String)>> = headers_table.pairs().collect::<Vec<_>>();
                    let mut result = HashMap::new();
                    for (k, v) in hdrs.into_iter().flatten() {
                        result.insert(k, v);
                    }
                    Some(result)
                }
                _ => None,
            }
        };
        handle_request(&url, &method, headers)
    } else {
        Err(Error::RuntimeError("Unsupported request argument".to_string()))
    }
}

fn handle_request(
    url: &str,
    method: &str,
    headers: Option<HashMap<String, String>>,
) -> Result<(Option<String>, u16), Error> {
    let client = Client::builder()
        .build()
        .map_err(|e| Error::RuntimeError(e.to_string()))?;
    let method = Method::try_from(method).map_err(|e| Error::RuntimeError(e.to_string()))?;
    let header_map = match headers {
        None => HeaderMap::new(),
        Some(headers) => {
            let mut header_map = HeaderMap::new();
            for (k, v) in headers {
                header_map.insert(
                    HeaderName::try_from(k).map_err(|e| Error::RuntimeError(e.to_string()))?,
                    v.parse()
                        .map_err(|_e| Error::RuntimeError("Failed parsing header".to_string()))?,
                );
            }
            header_map
        }
    };
    let req = client
        .request(method, url)
        .headers(header_map)
        .build()
        .map_err(|e| Error::RuntimeError(e.to_string()))?;
    let res = client.execute(req).map_err(|e| Error::RuntimeError(e.to_string()))?;
    let status_code = res.status().as_u16();
    let body = match res.text() {
        Ok(body) => Ok(body),
        Err(e) => Err(Error::RuntimeError(e.to_string())),
    }?;
    Ok((Some(body), status_code))
}

#[cfg(test)]
mod tests {
    use mlua::Lua;

    #[test]
    #[ignore]
    fn test_get_via_string_arg() {
        let lua = Lua::new();
        crate::preload(&lua).expect("Expected to preload module");
        let script = r#"
            return require('socket.http').request('https://apt.on-prem.net/public.key')
        "#;
        let s: String = lua.load(script).eval().expect("Expected to perform an https request");
        assert!(s.starts_with("-----BEGIN PGP PUBLIC KEY BLOCK-----\nVersion: GnuPG v1\n\n"));
    }

    #[test]
    #[ignore]
    fn test_get_via_table_arg() {
        let lua = Lua::new();
        crate::preload(&lua).expect("Expected to preload module");
        let script = r#"
            http = require('socket.http')
            return http.request({ url = 'https://apt.on-prem.net/public.key', method = 'GET' })
        "#;
        let s: String = lua
            .load(script)
            .eval()
            .expect("Expected to perform an https request with method=GET");
        assert!(s.starts_with("-----BEGIN PGP PUBLIC KEY BLOCK-----\nVersion: GnuPG v1\n\n"));
    }
}
