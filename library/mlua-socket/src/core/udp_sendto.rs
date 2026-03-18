use super::udp::Udp;
use mlua::{Error, FromLua, Lua, MultiValue};
use socket2::SockAddr;
use std::io::IoSlice;
use std::net::SocketAddr;

pub(super) fn handle(lua: &Lua, udp: &Udp, args: MultiValue) -> Result<usize, Error> {
    // Parse args
    let datagram: String = String::from_lua(args[0].clone(), lua)?;
    let addr: String = String::from_lua(args[1].clone(), lua)?;
    let port: u16 = u16::from_lua(args[2].clone(), lua)?;

    let socket_addr: SocketAddr = format!("{addr}:{port}").parse()?;
    let sock_addr: SockAddr = SockAddr::from(socket_addr);

    // Perform
    let bufs = &[IoSlice::new(datagram.as_bytes())];
    let msg = socket2::MsgHdr::new().with_addr(&sock_addr).with_buffers(bufs);
    let socket = udp.socket.lock().map_err(|err| Error::RuntimeError(err.to_string()))?;
    let _bytes_sent = socket.sendmsg(&msg, 0)?;
    Ok(1)
}

#[cfg(test)]
mod tests {
    use mlua::Lua;
    use std::net::SocketAddr;
    use std::sync::{Arc, Mutex};
    use tokio::net::UdpSocket;
    use tokio::time::{Duration, sleep};

    #[tokio::test]
    async fn sendto() {
        // Setup a server to receive a datagram
        let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let udp_socket = UdpSocket::bind(&addr)
            .await
            .expect("Expected to bind to loopback addr with a dynamic port assignment");
        let local_addr = udp_socket.local_addr().expect("Expected to obtain local address");
        let port = local_addr.port();
        let result = Arc::new(Mutex::new(vec![0; 3]));
        let result_clone = result.clone();
        tokio::spawn(async move {
            let mut buf = vec![0; 3];
            let (_len, _addr) = udp_socket.recv_from(&mut buf).await.unwrap();
            let mut locked_result = result_clone.lock().unwrap();
            *locked_result = buf;
        });

        // Test
        let lua = Lua::new();
        crate::preload(&lua).expect("Expected to preload module");
        let script = r#"
                local socket = require('socket')
                local udp = socket.udp()
                return udp:sendto('abc', '127.0.0.1', _port_)
            "#
        .replace("_port_", format!("{port}").as_str());
        let (status, err): (Option<u16>, Option<mlua::Value>) =
            lua.load(script).eval().expect("Expected to send a few chars");
        assert_eq!(err, None);
        assert_eq!(status, Some(1));
        sleep(Duration::from_millis(50)).await;
        let locked_result = result.lock().unwrap();
        let result_str = bstr::BString::from(locked_result.as_slice());
        assert_eq!(result_str, "abc");
    }
}
