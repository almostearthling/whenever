use super::tcp::Tcp;
use mlua::{Error, Lua};
use std::net::Shutdown;

pub(super) fn handle(_lua: &Lua, tcp: &Tcp) -> Result<(), Error> {
    let _ = tcp.socket.shutdown(Shutdown::Both);
    Ok(())
}

#[cfg(test)]
mod tests {
    use mlua::Lua;
    use std::net::SocketAddr;
    use tokio::io::AsyncReadExt;
    use tokio::net::TcpListener;

    #[tokio::test]
    async fn close() {
        // Setup a listener to connect to and a receiver
        let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let listener = TcpListener::bind(&addr)
            .await
            .expect("Expected to bind to loopback addr with a dynamic port assignment");
        let local_addr = listener.local_addr().expect("Expected to obtain local address");
        let port = local_addr.port();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = vec![0; 3];
            let _ = socket.read_exact(&mut buf).await;
        });

        // Test
        let lua = Lua::new();
        crate::preload(&lua).expect("Expected to preload module");
        let script = r#"
                local socket = require('socket')
                local master = socket.tcp()
                local ok, err = master:connect('127.0.0.1', _port_)
                assert(ok)
                master:close()
                return master:send('abc')
            "#
        .replace("_port_", format!("{port}").as_str());
        let (_bytes_sent, err): (Option<u16>, Option<String>) =
            lua.load(script).eval().expect("Expected to send a few characters");
        assert_ne!(err, None);
    }
}
