use std::io::Read;
use std::io::Error as IOError;
use std::net::{SocketAddr, ToSocketAddrs};
use std::path::PathBuf;
use std::ops::AddAssign;

use allreduce::allreduce_ring;
use communicator::Communicator;
use libkoala::Error as LibKoalaError;
use structopt::StructOpt;
use thiserror::Error;

pub mod communicator;
pub mod allreduce;

#[derive(Error, Debug)]
pub enum CommunicatorError {
    #[error("IO Error {0}")]
    Io(#[from] IOError),
    #[error("Koala error: {0}")]
    KoalaError(#[from] LibKoalaError),
    #[error("No address is resolved")]
    NoAddrResolved,
    #[error("Handshake failed: {0}")]
    HandshakeError(&'static str),
    #[error("Memory region error: {0}")]
    MemoryRegionError(&'static str),
    #[error("Invalid worker index")]
    InvalidWorkerIndex
}

#[derive(Error, Debug)]
pub enum AllReduceError {
    #[error("Invalid argument: {0}")]
    InvalidArgument(&'static str),
    #[error("Peer communicator error: {0}")]
    Communicator(#[from] CommunicatorError),
    #[error("Peer communicator error: {0}")]
    Koala(#[from] LibKoalaError)
}

const DEFAULT_PORT: &str = "6042";
const NUM_ELEMENTS: &str = "100";

#[derive(StructOpt, Debug, Clone)]
#[structopt(about = "Ring allreduce")]
pub struct Opts {
    /// The addresses of the workers, seperated by ','. Each can be an IP address or domain name.
    #[structopt(short, long)]
    pub hosts: Option<String>,

    /// The addresses of the workers. Each can be an IP address or domain name.
    #[structopt(short = "f", long)]
    pub hostfile: Option<PathBuf>,

    /// The port number to use if not given.
    #[structopt(short, long, default_value = DEFAULT_PORT)]
    pub port: u16,

    /// Index of current worker
    #[structopt(short, long)]
    pub index: usize,

    /// Chunk size in AllReduce
    pub chunk_size: Option<usize>,

    /// Number of elements in AllReduce
    #[structopt(short, long, default_value = NUM_ELEMENTS)]
    pub num_elements: usize,
}

fn get_host_list(opts: &Opts) -> Vec<SocketAddr> {
    assert!(
        opts.hosts.is_some() ^ opts.hostfile.is_some(),
        "Either hosts or hostfile must be non-empty."
    );

    let addrs: Vec<String> = if let Some(hosts) = &opts.hosts {
        hosts
            .split(',')
            .filter(|x| !x.is_empty())
            .map(String::from)
            .collect()
    } else {
        let mut file = std::fs::File::open(opts.hostfile.as_ref().unwrap()).expect("Failed to open hostfile");
        let mut content = String::new();
        file.read_to_string(&mut content).unwrap();
        content
            .split('\n')
            .map(|x| x.trim())
            .filter(|x| !x.is_empty() && !x.starts_with("#"))
            .map(String::from)
            .collect()
    };

    addrs
        .into_iter()
        .map(|x| {
            if !x.contains(':') {
                format!("{}:{}", x, opts.port)
            } else {
                x
            }
        })
        .map(|x| {
            // use the first resolved address
            x.to_socket_addrs()
                .unwrap()
                .next()
                .expect("Resolve address failed")
        })
        .collect()
}


#[derive(Debug, Copy, Clone, PartialEq)]
struct Point {
    x: i32,
    y: i32
}

impl AddAssign for Point {
    fn add_assign(&mut self, other: Self) {
        *self = Self {
            x: self.x + other.x,
            y: self.y + other.y,
        };
    }
}
fn main() {
    let opts = Opts::from_args();
    let addrs = get_host_list(&opts);
    let my_index = opts.index.to_owned();

    let communicator = Communicator::new(addrs, my_index).unwrap();

    let mut points = Vec::with_capacity(opts.num_elements);
    for i in 0..opts.num_elements {
        let point = Point {
            x: 42 + (i as i32) + (my_index as i32),
            y: (i as i32) + (my_index as i32)
        };
        points.push(point);
    }

    let result = allreduce_ring(&points[..], &communicator, opts.chunk_size).unwrap();

    println!("ring allreduce completed!");
    for (i, point) in result.iter().take(5).enumerate() {
        println!("Point {}: (x={}, y={})", i, point.x, point.y);
    }
}