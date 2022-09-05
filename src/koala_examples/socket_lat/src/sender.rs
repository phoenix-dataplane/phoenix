use socket2::{Domain, Socket, Type};
use std::mem::MaybeUninit;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

fn main() -> Result<(), std::io::Error> {
    let socket = Socket::new(Domain::IPV4, Type::STREAM, None)?;
    let address: SocketAddr = "192.168.211.194:8080".parse().unwrap();
    socket.connect(&address.into())?;
    socket.set_nodelay(true)?;
    // socket.set_nonblocking(true)?;

    const LEN: usize = 88;
    let mut array: [u8; LEN] = [0; LEN];
    for i in 0..array.len() {
        array[i] = i as _;
    }

    let mut buf: [u8; LEN] = [0; LEN];
    let buf = unsafe { &mut *(&mut buf[..] as *mut [u8] as *mut [MaybeUninit<u8>]) };

    std::thread::sleep(Duration::from_secs(1));

    let warmup = 100;
    let num = 1000;
    let mut lat = Vec::with_capacity(num);
    for i in 0..warmup + num {
        let start = Instant::now();
        socket.send(&array)?;
        socket.recv(buf)?;
        let end = Instant::now();
        if i >= warmup {
            lat.push(end.duration_since(start).as_nanos());
        }
    }
    lat.sort();

    println!(
        "min: {}, median: {}, P95: {}, P99: {}, max: {}",
        lat[0],
        lat[lat.len() / 2],
        lat[(lat.len() as f64 * 0.95) as usize],
        lat[(lat.len() as f64 * 0.99) as usize],
        lat[lat.len() - 1]
    );
    Ok(())
}
