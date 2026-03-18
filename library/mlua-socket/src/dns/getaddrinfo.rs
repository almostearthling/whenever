use dns_lookup::lookup_host;
use mlua::{Error, Lua, Table};

pub(super) fn handle(lua: &Lua, address: mlua::String) -> Result<Table, Error> {
    let address = address.to_str()?;
    let result = lua.create_table()?;
    for ip in lookup_host(&address)? {
        let entry = lua.create_table()?;
        entry.raw_set("family", if ip.is_ipv6() { "inet6" } else { "inet" })?;
        entry.raw_set("addr", ip.to_string())?;
        result.push(entry)?;
    }
    Ok(result)
}
