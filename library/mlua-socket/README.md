# mlua-socket

A Rust-native implementation of [LuaSocket](https://github.com/lunarmodules/luasocket)
for [mlua](https://crates.io/crates/mlua).

[![License](http://img.shields.io/badge/Licence-MIT-blue.svg)](LICENSE)
[![Arch](https://img.shields.io/badge/Arch-aarch64%20|%20amd64%20|%20armv7-blue.svg)]()
[![Lua](https://img.shields.io/badge/Lua-5.1%20|%205.2%20|%205.3%20|%205.4%20|%20LuaJIT%20|%20LuaJIT%205.2-blue.svg)]()

## Installing

Add to your Rust project using one of MLua's features: [lua51, lua52, lua53, lua54, luajit, luajit52].

```shell
$ cargo add mlua-socket --features luajit
```

## Testing

```shell
$ make check
```

## Benchmarking

```shell
$ make bench
```

## Using

```rust
use mlua::Lua;

let lua = Lua::new();
mlua_socket::preload(&lua);
let script = r#"
    local socket = require('socket')
    local client = socket.connect('127.0.0.1', 3000)
    return client:send('abcd')
"#;
let _last_index: u16 = lua.load(script).eval()?;
```
