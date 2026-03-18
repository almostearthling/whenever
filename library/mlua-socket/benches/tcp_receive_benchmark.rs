use criterion::{Criterion, Throughput, criterion_group, criterion_main};
extern crate tokio;
use mlua::Lua;
use rand::Rng;
use rand::distr::Alphanumeric;
use std::net::SocketAddr;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tokio::time::{Duration, sleep};

fn receive_all(lua: &Lua, port: u16) {
    let script = r#"
                local socket = require('socket')
                local master = socket.tcp()
                local ok, err = master:connect('127.0.0.1', _port_)
                assert(ok)
                return master:receive('*a')
            "#
    .replace("_port_", format!("{port}").as_str());
    let _data: String = lua.load(script).eval().unwrap();
}

#[tokio::main(flavor = "multi_thread")]
async fn bench(c: &mut Criterion) {
    let data: String = rand::rng()
        .sample_iter(&Alphanumeric)
        .take(1_000_000)
        .map(char::from)
        .collect();

    // Setup a listener to connect to and a receiver
    let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
    let listener = TcpListener::bind(&addr).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let data_clone = data.clone();
    tokio::spawn(async move {
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            let _ = socket.write_all(data_clone.as_bytes()).await.unwrap();
            socket.flush().await.unwrap();
            socket.shutdown().await.unwrap();
        }
    });
    sleep(Duration::from_millis(50)).await;

    // Benchmark
    let lua = Lua::new();
    mlua_socket::preload(&lua).unwrap();
    let mut group = c.benchmark_group("tcp receive 1mb");
    group.sample_size(10);
    group.throughput(Throughput::Bytes(data.len() as u64));
    group.bench_function("receive_all()", |b| b.iter(|| receive_all(&lua, port)));
    group.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
