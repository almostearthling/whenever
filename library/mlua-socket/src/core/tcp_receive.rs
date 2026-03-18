use super::tcp::Tcp;
use mlua::{Error, FromLua, Lua, MultiValue};
use std::mem::MaybeUninit;

unsafe fn assume_init(buf: &[MaybeUninit<u8>]) -> &[u8] {
    unsafe { &*(buf as *const [MaybeUninit<u8>] as *const [u8]) }
}

pub(super) fn handle(lua: &Lua, tcp: &Tcp, args: MultiValue) -> Result<Vec<u8>, Error> {
    // Parse args
    let pattern = {
        if args.is_empty() {
            "*l".to_string()
        } else {
            String::from_lua(args[0].clone(), lua)?
        }
    };
    let prefix: Option<String> = {
        if args.len() < 2 {
            None
        } else {
            Some(String::from_lua(args[1].clone(), lua)?)
        }
    };

    // Perform
    if pattern == "*a" {
        receive_all(lua, tcp, prefix)
    } else if pattern == "*l" {
        receive_line(lua, tcp, prefix)
    } else {
        let num_bytes = pattern
            .parse::<usize>()
            .map_err(|err| Error::RuntimeError(err.to_string()))?;
        receive_num_bytes(lua, tcp, num_bytes, prefix)
    }
}

fn receive_line(_lua: &Lua, tcp: &Tcp, prefix: Option<String>) -> Result<Vec<u8>, Error> {
    let mut line: Vec<u8> = Vec::with_capacity(8_000);
    if let Some(prefix) = prefix {
        for b in prefix.as_bytes() {
            line.push(*b);
        }
    }
    let mut char_buf = [MaybeUninit::new(0); 1];
    loop {
        let (bytes_read, _addr) = tcp.socket.recv_from(&mut char_buf)?;
        if bytes_read < 1 {
            break;
        }
        let bytes = unsafe { assume_init(&char_buf) };
        let c = bytes[0];
        if c == b'\n' {
            break;
        } else if c == b'\r' {
            continue;
        }
        line.push(c);
    }
    Ok(line)
}

fn receive_num_bytes(_lua: &Lua, tcp: &Tcp, num_bytes: usize, prefix: Option<String>) -> Result<Vec<u8>, Error> {
    let mut result_buf: Vec<u8> = Vec::with_capacity(num_bytes);
    if let Some(prefix) = prefix {
        for b in prefix.as_bytes() {
            result_buf.push(*b);
        }
    }
    let mut char_buf = [MaybeUninit::new(0); 1];
    while result_buf.len() < num_bytes {
        let (bytes_read, _addr) = tcp.socket.recv_from(&mut char_buf)?;
        if bytes_read < 1 {
            break;
        }
        let bytes = unsafe { assume_init(&char_buf) };
        let c = bytes[0];
        result_buf.push(c);
    }
    Ok(result_buf)
}

fn receive_all(_lua: &Lua, tcp: &Tcp, prefix: Option<String>) -> Result<Vec<u8>, Error> {
    let mut result_buf: Vec<u8> = Vec::with_capacity(8_000);
    if let Some(prefix) = prefix {
        for b in prefix.as_bytes() {
            result_buf.push(*b);
        }
    }
    let mut buf = [MaybeUninit::new(0); 8_000];
    loop {
        let (bytes_read, _addr) = tcp.socket.recv_from(&mut buf)?;
        if bytes_read < 1 {
            break;
        }
        let bytes = unsafe { assume_init(&buf) };
        for c in bytes.iter().take(bytes_read) {
            result_buf.push(*c);
        }
    }
    Ok(result_buf)
}

#[cfg(test)]
mod tests {
    use mlua::Lua;
    use std::net::SocketAddr;
    use tokio::io::AsyncWriteExt;
    use tokio::net::TcpListener;
    use tokio::time::{Duration, sleep};

    #[tokio::test(flavor = "multi_thread")]
    async fn receive_all_1_mb() {
        let data: Vec<u8> = (0..1_000_000).map(|_| rand::random::<u8>()).collect();

        // Setup a listener to connect to and a receiver
        let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let listener = TcpListener::bind(&addr)
            .await
            .expect("Expected to bind to loopback address with dynamic port assignment");
        let local_addr = listener.local_addr().expect("Expected to obtain local address");
        let port = local_addr.port();
        let data_clone = data.clone();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let _ = socket.write_all(data_clone.as_slice()).await.unwrap();
            socket.flush().await.unwrap();
            socket.shutdown().await.unwrap();
        });
        sleep(Duration::from_millis(50)).await;

        // Test
        let lua = Lua::new();
        crate::preload(&lua).expect("Expected to preload module");
        let script = r#"
                local socket = require('socket')
                local master = socket.tcp()
                local ok, err = master:connect('127.0.0.1', _port_)
                assert(ok)
                return master:receive('*a')
            "#
        .replace("_port_", format!("{port}").as_str());
        let received: bstr::BString = lua.load(script).eval().expect("Expected to receive all");
        assert_eq!(received.to_vec(), data);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn receive_all_with_prefix() {
        // Setup a listener to connect to and a receiver
        let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let listener = TcpListener::bind(&addr)
            .await
            .expect("Expected to bind to loopback address with dynamic port assignment");
        let local_addr = listener.local_addr().expect("Expected to obtain local address");
        let port = local_addr.port();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let _ = socket.write_all(b"abc\n").await.unwrap();
            socket.flush().await.unwrap();
            let _ = socket.write_all(b"123\n").await.unwrap();
            socket.flush().await.unwrap();
            socket.shutdown().await.unwrap();
        });
        sleep(Duration::from_millis(50)).await;

        // Test
        let lua = Lua::new();
        crate::preload(&lua).expect("Expected to preload module");
        let script = r#"
                local socket = require('socket')
                local master = socket.tcp()
                local ok, err = master:connect('127.0.0.1', _port_)
                assert(ok)
                return master:receive('*a', 'xyz\n')
            "#
        .replace("_port_", format!("{port}").as_str());
        let received: bstr::BString = lua.load(script).eval().expect("Expected to receive all with prefix");
        assert_eq!(received.to_string(), "xyz\nabc\n123\n");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn receive_line() {
        // Setup a listener to connect to and a receiver
        let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let listener = TcpListener::bind(&addr)
            .await
            .expect("Expected to bind to loopback address with dynamic port assignment");
        let local_addr = listener.local_addr().expect("Expected to obtain local address");
        let port = local_addr.port();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let _ = socket.write_all(b"abc123\n").await.unwrap();
            socket.flush().await.unwrap();
        });
        sleep(Duration::from_millis(50)).await;

        // Test
        let lua = Lua::new();
        crate::preload(&lua).expect("Expected to preload module");
        let script = r#"
                local socket = require('socket')
                local master = socket.tcp()
                local ok, err = master:connect('127.0.0.1', _port_)
                assert(ok)
                return master:receive()
            "#
        .replace("_port_", format!("{port}").as_str());
        let line: String = lua.load(script).eval().expect("Expected to receive line");
        assert_eq!(line, "abc123");
    }
}
