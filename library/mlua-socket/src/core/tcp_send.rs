use super::tcp::Tcp;
use mlua::{Error, FromLua, Lua, MultiValue};

pub(super) fn handle(lua: &Lua, tcp: &Tcp, args: MultiValue) -> Result<usize, Error> {
    // Parse args. arg1:start and arg2:end are 1-based indexes into arg0:data
    let data: String = String::from_lua(args[0].clone(), lua)?;
    let start: usize = {
        if args.len() >= 2 {
            usize::from_lua(args[1].clone(), lua)?
        } else {
            1
        }
    };
    let end: usize = {
        if args.len() >= 3 {
            usize::from_lua(args[2].clone(), lua)?
        } else {
            data.len()
        }
    };
    let raw_data = &data.as_bytes()[&start - 1..end];

    // Send
    let bytes_sent = tcp.socket.send(raw_data)?;
    Ok(bytes_sent + start - 1)
}

#[cfg(test)]
mod tests {
    use mlua::Lua;
    use std::net::SocketAddr;
    use std::sync::{Arc, Mutex};
    use tokio::io::AsyncReadExt;
    use tokio::net::TcpListener;
    use tokio::time::{Duration, sleep};

    #[tokio::test]
    async fn send() {
        // Setup a listener to connect to and a receiver
        let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let listener = TcpListener::bind(&addr)
            .await
            .expect("Expected to bind to loopback address with dynamic port assignment");
        let local_addr = listener.local_addr().expect("Expected to obtain local address");
        let port = local_addr.port();
        let result = Arc::new(Mutex::new(vec![0; 3]));
        let result_clone = result.clone();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = vec![0; 3];
            let _ = socket.read_exact(&mut buf).await;
            let mut locked_result = result_clone.lock().unwrap();
            *locked_result = buf;
        });

        // Test
        let lua = Lua::new();
        crate::preload(&lua).expect("Expected to preload module");
        let script = r#"
                local socket = require('socket')
                local master = socket.tcp()
                local ok, err = master:connect('127.0.0.1', _port_)
                assert(ok)
                return master:send('abc')
            "#
        .replace("_port_", format!("{port}").as_str());
        let bytes_sent: u16 = lua.load(script).eval().expect("Expected to send a few chars");
        assert_eq!(bytes_sent, 3);
        sleep(Duration::from_millis(50)).await;
        let locked_result = result.lock().unwrap();
        let result_str = bstr::BString::from(locked_result.as_slice());
        assert_eq!(result_str, "abc");
    }

    #[tokio::test]
    async fn send_with_start() {
        // Setup a listener to connect to and a receiver
        let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let listener = TcpListener::bind(&addr)
            .await
            .expect("Expected to bind to loopback address with dynamic port assignment");
        let local_addr = listener.local_addr().expect("Expected to obtain local address");
        let port = local_addr.port();
        let result = Arc::new(Mutex::new(vec![0; 3]));
        let result_clone = result.clone();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = vec![0; 3];
            let _ = socket.read_exact(&mut buf).await;
            let mut locked_result = result_clone.lock().unwrap();
            *locked_result = buf;
        });

        // Test
        let lua = Lua::new();
        crate::preload(&lua).expect("Expected to preload module");
        let script = r#"
                local socket = require('socket')
                local master = socket.tcp()
                local ok, err = master:connect('127.0.0.1', _port_)
                assert(ok)
                return master:send('abcd', 2)
            "#
        .replace("_port_", format!("{port}").as_str());
        let last_index: u16 = lua.load(script).eval().expect("Expected to send a few chars");
        assert_eq!(last_index, 4);
        sleep(Duration::from_millis(50)).await;
        let locked_result = result.lock().unwrap();
        let result_str = bstr::BString::from(locked_result.as_slice());
        assert_eq!(result_str, "bcd");
    }

    #[tokio::test]
    async fn send_with_start_and_end() {
        // Setup a listener to connect to and a receiver
        let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let listener = TcpListener::bind(&addr)
            .await
            .expect("Expected to bind to loopback address with dynamic port assignment");
        let local_addr = listener.local_addr().expect("Expected to obtain local address");
        let port = local_addr.port();
        let result = Arc::new(Mutex::new(String::new()));
        let result_clone = result.clone();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = vec![0; 10];
            let bytes_read = socket.read(&mut buf).await.unwrap();
            let mut locked_result = result_clone.lock().unwrap();
            *locked_result = bstr::BString::from(&buf[0..bytes_read]).to_string();
        });

        // Test
        let lua = Lua::new();
        crate::preload(&lua).expect("Expected to preload module");
        let script = r#"
                local socket = require('socket')
                local master = socket.tcp()
                local ok, err = master:connect('127.0.0.1', _port_)
                assert(ok)
                return master:send('abcd', 2, 3)
            "#
        .replace("_port_", format!("{port}").as_str());
        let last_index: u16 = lua.load(script).eval().expect("Expected to send a few chars");
        assert_eq!(last_index, 3);
        sleep(Duration::from_millis(50)).await;
        let locked_result = result.lock().unwrap();
        assert_eq!(*locked_result, "bc");
    }
}
