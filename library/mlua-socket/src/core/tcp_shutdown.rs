use super::tcp::Tcp;
use mlua::{Error, Lua, MultiValue};
use std::net::Shutdown;

pub(super) fn handle(_lua: &Lua, tcp: &Tcp, args: MultiValue) -> Result<(), Error> {
    let mode: Shutdown = {
        if args.is_empty() {
            Shutdown::Both
        } else {
            let arg0 = args[0].to_string()?;
            match arg0.as_str() {
                "send" => Shutdown::Write,
                "receive" => Shutdown::Read,
                _ => Shutdown::Both,
            }
        }
    };
    tcp.socket
        .shutdown(mode)
        .map_err(|err| Error::RuntimeError(err.to_string()))?;
    Ok(())
}

#[cfg(feature = "server")]
#[cfg(test)]
mod tests {
    use mlua::Lua;
    use std::net::SocketAddr;
    use tokio::io::AsyncReadExt;
    use tokio::net::TcpListener;

    #[tokio::test]
    async fn shutdown() {
        // Setup a listener to connect to and a receiver
        let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let listener = TcpListener::bind(&addr)
            .await
            .expect("Expected to bind to loopback address with dynamic port assignment");
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
                local client = socket.connect('127.0.0.1', _port_)
                client:shutdown()
                return client:send('abc')
            "#
        .replace("_port_", format!("{port}").as_str());
        let (_bytes_sent, err): (Option<u16>, Option<String>) =
            lua.load(script).eval().expect("Expected to send a few chars");
        assert_ne!(err, None);
    }
}
