use std::collections::HashMap;
use std::error::Error;
use std::io::{IoSlice, Read, Write};
use std::os::unix::io::AsRawFd;

use mio::net::{TcpListener, TcpStream};
use mio::{Events, Interest, Poll, Token};
use structopt::StructOpt;

#[derive(StructOpt, Debug, Clone)]
#[structopt(about = "Socket lat test")]
pub struct Args {
    #[structopt(short = "c", long = "connect", default_value = "")]
    pub ip: String,

    /// The port number to use.
    #[structopt(short, long, default_value = "5000")]
    pub port: u16,

    /// Request size
    #[structopt(short, long, default_value = "64")]
    pub data_size: usize,

    /// Total number of iterations.
    #[structopt(short, long, default_value = "16384")]
    pub total_iters: usize,

    /// Number of warmup iterations.
    #[structopt(short, long, default_value = "1000")]
    pub warmup: usize,

    #[structopt(short, long)]
    pub iovec: bool,
}

pub struct Endpoint {
    pub poll: Poll,
    pub events: Events,
    pub listener: Option<TcpListener>,
    pub socks: HashMap<usize, TcpStream>,
    pub now: std::time::Instant,
    pub header: Vec<u8>,
    pub buf: Vec<u8>,
    pub read_offset: usize,
    pub latencies: Vec<std::time::Duration>,
}

fn send(sock: &mut TcpStream, buf: &[u8]) -> Result<usize, std::io::Error> {
    let mut nbytes = 0;
    while nbytes < buf.len() {
        match sock.write(buf) {
            Ok(n) => nbytes += n,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => continue,
            Err(e) => return Err(e),
        }
    }
    // println!("len {} nbytes {}", buf.len(), nbytes);
    Ok(nbytes)
}

fn recv(sock: &mut TcpStream, buf: &mut [u8]) -> Result<usize, std::io::ErrorKind> {
    match sock.read(buf) {
        Ok(0) => Err(std::io::ErrorKind::ConnectionReset),
        Ok(n) => Ok(n),
        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => Ok(0),
        Err(_e) => Err(std::io::ErrorKind::ConnectionReset),
    }
}

/*
fn recv(sock: &mut TcpStream, buf: &mut [u8]) -> Result<usize, std::io::Error> {
    let mut nbytes = 0;
    loop {
        match sock.read(buf) {
            Ok(0) => break,
            Ok(n) => nbytes += n,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
            Err(e) => return Err(e),
        }
    }
    Ok(nbytes)
}
*/

const HEADER_LEN: usize = 16;

fn run(i: usize, args: &Args, ep: &mut Endpoint) -> Result<usize, std::io::Error> {
    let mut progress = 0;
    ep.poll.poll(&mut ep.events, None)?;

    for event in ep.events.iter() {
        let handle = event.token().0;
        if handle == 0 {
            let (mut sock, addr) = ep.listener.as_ref().unwrap().accept()?;
            sock.set_nodelay(true)?;
            println!("New connection from {}", addr);
            let handle = sock.as_raw_fd() as _;
            ep.poll
                .registry()
                .register(&mut sock, Token(handle), Interest::READABLE)?;
            ep.socks.insert(handle, sock);
        } else {
            let sock = ep.socks.get_mut(&handle).unwrap();

            if event.is_writable() {
                if ep.listener.is_none() && i > args.warmup {
                    ep.now = std::time::Instant::now();
                }

                if args.iovec {
                    sock.write_vectored(&[IoSlice::new(&ep.header), IoSlice::new(&ep.buf)])?;
                } else {
                    let len = HEADER_LEN + args.data_size;
                    if len <= 1024 {
                        let mut combined = vec![0; len];
                        unsafe {
                            std::ptr::copy(ep.header.as_ptr(), combined.as_mut_ptr(), HEADER_LEN);
                        }
                        unsafe {
                            std::ptr::copy(
                                ep.buf.as_ptr(),
                                combined.as_mut_ptr().offset(HEADER_LEN as _),
                                args.data_size,
                            );
                        }
                        if let Err(e) = send(sock, &combined) {
                            println!("Write: {}", e);
                            continue;
                        }
                    } else {
                        if let Err(e) = send(sock, &ep.header[0..HEADER_LEN]) {
                            println!("Write: {}", e);
                            continue;
                        }
                        if let Err(e) = send(sock, &ep.buf[0..args.data_size]) {
                            println!("Write: {}", e);
                            continue;
                        }
                    }
                }

                ep.poll
                    .registry()
                    .reregister(sock, Token(handle), Interest::READABLE)?;
            }
            if event.is_readable() {
                let mut would_block = false;
                let mut error = false;
                loop {
                    if ep.read_offset < HEADER_LEN {
                        if let Ok(n) = recv(sock, &mut ep.header[ep.read_offset..HEADER_LEN]) {
                            if n == 0 {
                                would_block = true;
                                break;
                            }
                            ep.read_offset += n;
                            if ep.read_offset < HEADER_LEN {
                                continue;
                            }
                        } else {
                            error = true;
                            break;
                        }
                    }
                    if ep.read_offset < HEADER_LEN + args.data_size {
                        if let Ok(n) = recv(
                            sock,
                            &mut ep.buf[(ep.read_offset - HEADER_LEN)..args.data_size],
                        ) {
                            if n == 0 {
                                would_block = true;
                                break;
                            }
                            ep.read_offset += n;
                        } else {
                            error = true;
                            break;
                        }
                    }
                    if ep.read_offset == HEADER_LEN + args.data_size {
                        ep.read_offset = 0;
                        break;
                    }
                }
                if error {
                    println!("Disconnected");
                    // poll.registry().deregister(sock)?;
                } else if !would_block {
                    ep.poll
                        .registry()
                        .reregister(sock, Token(handle), Interest::WRITABLE)?;
                    if ep.listener.is_none() {
                        if i > args.warmup {
                            ep.latencies.push(ep.now.elapsed());
                        }
                        progress += 1;
                    }
                }
            }
        }
    }
    Ok(progress)
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::from_args();
    println!("args {:?}", args);

    let mut ep = Endpoint {
        poll: Poll::new()?,
        events: Events::with_capacity(128),
        listener: None,
        socks: HashMap::new(),
        now: std::time::Instant::now(),
        header: vec![0; HEADER_LEN],
        buf: vec![0; args.data_size],
        read_offset: 0,
        latencies: Vec::new(),
    };

    if args.ip == "" {
        //server
        let addr = ("0.0.0.0:".to_owned() + &args.port.to_string()).parse()?;
        ep.listener = Some(TcpListener::bind(addr)?);
        ep.poll
            .registry()
            .register(ep.listener.as_mut().unwrap(), Token(0), Interest::READABLE)?;
    } else {
        // client
        let addr = (args.ip.to_owned() + ":" + &args.port.to_string()).parse()?;
        let mut sock = TcpStream::connect(addr)?;
        let handle = sock.as_raw_fd() as usize;
        sock.set_nodelay(true)?;
        ep.poll
            .registry()
            .register(&mut sock, Token(handle), Interest::WRITABLE)?;
        /*
        loop {
            ep.poll.poll(&mut ep.events, None)?;
            match sock.peer_addr() {
                Ok(_) => {
                    ep.poll
                        .registry()
                        .register(&mut sock, Token(handle), Interest::WRITABLE)?;
                    break;
                }
                Err(err)
                    if err.kind() == std::io::ErrorKind::NotConnected
                        || err.raw_os_error() == Some(libc::EINPROGRESS) =>
                {
                    ep.poll
                        .registry()
                        .reregister(&mut sock, Token(handle), Interest::WRITABLE)?;
                    continue;
                }
                Err(e) => panic!("{}", e),
            }
        }
        */
        ep.socks.insert(handle, sock);
    }

    if ep.listener.is_some() {
        let mut i = 0;
        loop {
            run(i, &args, &mut ep)?;
            i += 1;
        }
    } else {
        let mut i = 0;
        while i < args.warmup + args.total_iters {
            i += run(i, &args, &mut ep)?;
        }

        let latencies = &mut ep.latencies;
        latencies.sort();
        println!(
            "{}, mean:{}us, min: {:?}, median: {:?} p95: {:?}, p99: {:?}, max: {:?}",
            latencies.len(),
            latencies.iter().map(|x| x.as_nanos()).sum::<u128>() as f64
                / (latencies.len() * 1000) as f64,
            latencies[0],
            latencies[(latencies.len() as f64 * 0.5) as usize],
            latencies[(latencies.len() as f64 * 0.95) as usize],
            latencies[(latencies.len() as f64 * 0.99) as usize],
            latencies[latencies.len() - 1]
        );
    }
    Ok(())
}
