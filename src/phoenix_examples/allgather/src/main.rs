use std::io::Read;
use std::net::{SocketAddr, ToSocketAddrs};
use std::path::PathBuf;
use std::thread;
use std::time;

use structopt::clap::arg_enum;
use structopt::StructOpt;

use libphoenix::cm;
use libphoenix::verbs::{MemoryRegion, SendFlags, WcStatus};

const DEFAULT_PORT: &str = "6000";

#[derive(StructOpt, Debug, Clone)]
#[structopt(about = "Allgather benchmark.")]
pub struct Opts {
    /// The addresses of the workers, seperated by ','. Each can be an IP address or domain name.
    #[structopt(short, long)]
    pub hosts: String,

    /// The port number to use if not given.
    #[structopt(short, long, default_value = DEFAULT_PORT)]
    pub port: u16,

    /// Total number of iterations.
    #[structopt(short, long, default_value = "1000")]
    pub total_iters: usize,

    /// Number of warmup iterations.
    #[structopt(short, long, default_value = "100")]
    pub warm_iters: usize,

    /// Message size.
    #[structopt(short, long, default_value = "65536")]
    pub msg_size: usize,
}

fn get_host_list(opts: &Opts) -> Vec<SocketAddr> {
    let hosts = &opts.hosts;
    let addrs: Vec<String> = 
        hosts
            .split(',')
            .filter(|x| !x.is_empty())
            .map(String::from)
            .collect();

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
                .expect("resolve address failed")
        })
        .collect()
}

#[derive(Debug)]
struct Communicator {
    send_id: cm::CmId,
    recv_id: cm::CmId,
    send_mr: MemoryRegion<u8>,
    recv_mr: MemoryRegion<u8>,
}

impl Communicator {
    fn new(opts: &Opts) -> anyhow::Result<Self> {
        let hosts = get_host_list(opts);
        // find myself, get the rank and the addr to bind
        let (rank, listen_addr) = hosts
            .iter()
            .enumerate()
            .find(|(_, addr)| cm::CmIdListener::bind(addr).is_ok())
            .expect("This host is not included in the host list");

        let left_peer_addr = &hosts[(rank + hosts.len() - 1) % hosts.len()];
        let (send_id, recv_id, recv_mr) = {
            if rank == hosts.len() - 1 {
                let (recv_id, recv_mr) = Communicator::connect(left_peer_addr, opts.msg_size)?;
                let send_id = Communicator::accept(listen_addr, opts.msg_size)?;
                (send_id,recv_id,recv_mr)
            } else{
                let send_id = Communicator::accept(listen_addr, opts.msg_size)?;
                let (recv_id, recv_mr) = Communicator::connect(left_peer_addr, opts.msg_size)?;
                (send_id,recv_id,recv_mr)
            }
        };


        //send_mr for node with rank + 1
        let send_mr = {
            let mut mr = send_id.alloc_msgs(opts.msg_size)?;
            mr.fill(42u8);
            mr
        };

        Ok(Communicator {
            send_id,
            recv_id,
            send_mr,
            recv_mr,
        })
    }

    fn accept (listen_addr:&SocketAddr, msg_size:usize) -> Result<cm::CmId, libphoenix::Error>{
        // create a listener
        let listener = cm::CmIdListener::bind(listen_addr)?;

        //accept node with rank + 1
        println!("Socket is listening for connection");
        let mut builder = listener.get_request()?;
        let pre_id = builder.set_max_send_wr(2).set_max_recv_wr(2).build()?;

        // allocate messages before accepting
        let mut recv_mr_temp: MemoryRegion<u8> = pre_id.alloc_msgs(msg_size)?;
        for _ in 0..2 {
            unsafe {
                pre_id.post_recv(&mut recv_mr_temp, .., 0)?;
            }
        }
        //socket accepts connections
        println!("Socket accepted connection");
        pre_id.accept(None)
    }

    fn connect(peer_addr:&SocketAddr, msg_size:usize)->Result<(cm::CmId,MemoryRegion<u8>), libphoenix::Error> {
        //connect to node with rank - 1
        let mut retry = 0;
        println!("Attempting connection to {:?}",peer_addr);
        let (recv_id, recv_mr) = loop {
            let builder = cm::CmIdBuilder::new()
                .set_max_recv_wr(2)
                .set_max_recv_wr(2)
                .resolve_route(peer_addr)?;

            let pre_id = builder.build()?;

            // allocate messages before connecting
            let mut recv_mr: MemoryRegion<u8> = pre_id.alloc_msgs(msg_size)?;
            for _ in 0..2 {
                unsafe {
                    pre_id.post_recv(&mut recv_mr, .., 0)?;
                }
            }

            match pre_id.connect(None) {
                Ok(id) => {
                    println!("Successful connection");
                    break (id, recv_mr)
                }
                Err(libphoenix::Error::Connect(e)) => {
                    println!("connect error: {}, connect retrying...", e);
                    thread::sleep(time::Duration::from_millis(100));
                    retry += 1;
                    if retry >= 50 {
                        return Err(libphoenix::Error::Connect(e));
                    }
                }
                Err(e) => panic!("connect: {}", e),
            }
        };
        Ok((recv_id, recv_mr))
    }
}


fn main() {
    let opts = Opts::from_args();
    let comm = Communicator::new(&opts).expect("Create communicator failed");
    println!("{:?}",comm);
}

