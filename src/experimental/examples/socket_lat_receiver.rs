use socket2::{Domain, Socket, Type};
use std::string::ToString;
use std::{mem::MaybeUninit, net::SocketAddr};

fn main() -> Result<(), std::io::Error> {
    let listener = Socket::new(Domain::IPV4, Type::STREAM, None)?;
    let address: SocketAddr = "0.0.0.0:8080".parse().unwrap();
    println!("Listening on {}", address.to_string());
    listener.bind(&address.into())?;
    listener.listen(128)?;

    loop {
        let (socket, addr) = listener.accept()?;
        socket.set_nodelay(true)?;
        println!(
            "New connection from {}",
            addr.as_socket().unwrap().to_string()
        );

        const LEN: usize = 128;
        let mut buf: [u8; LEN] = [0; LEN];
        let buf = unsafe { &mut *(&mut buf[..] as *mut [u8] as *mut [MaybeUninit<u8>]) };

        loop {
            if let Err(_e) = socket.recv(buf) {
                break;
            };
            if let Err(_e) = socket.send(b"ack") {
                break;
            };
        }
    }
}
