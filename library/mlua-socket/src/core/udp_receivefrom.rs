use super::udp::Udp;
use mlua::{Error, FromLua, Lua, MultiValue};
use socket2::MaybeUninitSlice;
use std::cmp::max;
use std::mem::MaybeUninit;

unsafe fn assume_init(buf: &[MaybeUninit<u8>]) -> &[u8] {
    unsafe { &*(buf as *const [MaybeUninit<u8>] as *const [u8]) }
}

pub(super) fn handle(lua: &Lua, udp: &Udp, args: MultiValue) -> Result<(Vec<u8>, String, u16), Error> {
    // Parse args
    let size: usize = {
        if !args.is_empty() {
            usize::from_lua(args[0].clone(), lua)?
        } else {
            8_192
        }
    };

    // Perform
    let mut buf = [MaybeUninit::new(0); 8_192];
    let socket = udp.socket.lock().map_err(|err| Error::RuntimeError(err.to_string()))?;
    let (bytes_received, _flags, addr) = socket.recv_from_vectored(&mut [MaybeUninitSlice::new(&mut buf)])?;
    let mut datagram: Vec<u8> = Vec::with_capacity(8_192);
    let bytes: &[u8] = unsafe { assume_init(&buf) };
    for c in bytes.iter().take(max(bytes_received, size)) {
        datagram.push(*c);
    }
    let socket_addr = match addr.as_socket() {
        Some(socket_addr) => socket_addr,
        None => return Err(Error::RuntimeError("Could not get socket addr".to_string())),
    };
    Ok((datagram, socket_addr.ip().to_string(), socket_addr.port()))
}
