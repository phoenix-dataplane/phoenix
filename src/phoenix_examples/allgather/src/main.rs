use core::num;
use std::net::{SocketAddr, ToSocketAddrs};
use std::thread;
use std::time;

use structopt::StructOpt;

use libphoenix::cm;
use libphoenix::verbs::{MemoryRegion, WcStatus, SendFlags};

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
    #[structopt(short, long, default_value = "10")]
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
    recv_mrs: Vec<MemoryRegion<u8>>,
    rank: usize,
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
        let (send_id, recv_id, recv_mrs) = {
            if rank == hosts.len() - 1 {
                let (recv_id, recv_mrs) = Communicator::connect(left_peer_addr, opts.msg_size, hosts.len() - 1)?;
                let send_id = Communicator::accept(listen_addr, opts.msg_size)?;
                (send_id,recv_id,recv_mrs)
            } else{
                let send_id = Communicator::accept(listen_addr, opts.msg_size)?;
                let (recv_id, recv_mrs) = Communicator::connect(left_peer_addr, opts.msg_size, hosts.len() - 1)?;
                (send_id,recv_id,recv_mrs)
            }
        };


        //send_mr for node with rank + 1
        let send_mr = {
            let mut mr = send_id.alloc_msgs(opts.msg_size)?;
            mr.fill((rank + 1) as u8);
            mr
        };

        Ok(Communicator {
            send_id,
            recv_id,
            send_mr,
            recv_mrs,
            rank
        })
    }

    fn accept (listen_addr:&SocketAddr, msg_size:usize) ->  Result<cm::CmId, libphoenix::Error>{
        // create a listener
        let listener = cm::CmIdListener::bind(listen_addr)?;

        //accept node with rank + 1
        println!("Socket is listening for connection");
        let mut builder = listener.get_request()?;
        let pre_id = builder.set_max_send_wr(2).set_max_recv_wr(2).build()?;

        // allocate messages before accepting            
        // allocate messages before connecting
        let mut recv_mr: MemoryRegion<u8> = pre_id.alloc_msgs(msg_size)?;
        for _ in 0..2 {
            unsafe {
                pre_id.post_recv(&mut recv_mr, .., 0)?;
            }
        }
        //socket accepts connections
        println!("Socket accepted connection");
        pre_id.accept(None)
    }

    fn connect(peer_addr:&SocketAddr, msg_size:usize, num_peers:usize)->Result<(cm::CmId,Vec<MemoryRegion<u8>>), libphoenix::Error> {
        //connect to node with rank - 1
        let mut retry = 0;
        println!("Attempting connection to {:?}",peer_addr);
        let (recv_id,recv_mrs) = loop {
            let builder = cm::CmIdBuilder::new()
                .set_max_recv_wr(2)
                .set_max_recv_wr(2)
                .resolve_route(peer_addr)?;
            let pre_id = builder.build()?;
            let mut recv_mrs = Vec::with_capacity(num_peers);
            // allocate messages before connecting
            for _ in 0..num_peers{
                let mut recv_mr: MemoryRegion<u8> = pre_id.alloc_msgs(msg_size)?;
                for _ in 0..2 {
                    unsafe {
                        pre_id.post_recv(&mut recv_mr, .., 0)?;
                    }
                }
                recv_mrs.push(recv_mr);
            }
            

            match pre_id.connect(None) {
                Ok(id) => {
                    println!("Successful connection");
                    break (id,recv_mrs)
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
        Ok((recv_id,recv_mrs))
    }
}

fn run_single_thread (opts: &Opts, mut comm: Communicator) -> anyhow::Result<()> {
    let num_hosts = get_host_list(opts).len();
    
    for i in 0..(num_hosts - 1){
        let recv_rank = (comm.rank + num_hosts - 1) % num_hosts;

        println!("{}",i);

        let send_mr = if i == 0 {
                let mut mr = comm.send_id.alloc_msgs(opts.msg_size)?;
                mr.copy_from_slice(comm.send_mr.as_slice());
                mr
            }
            else {
            let mut mr = comm.send_id.alloc_msgs(opts.msg_size)?;
            mr.copy_from_slice(comm.recv_mrs[i-1].as_slice());
            mr
        };
        println!("send data:  {:?}", send_mr.as_slice());
        println!("send mr made");
        unsafe{
            comm.send_id.post_send(&send_mr, .., i as u64, SendFlags::SIGNALED)?;
        }
        println!("post send");

        println!("get send comp");
        let wc = comm.send_id.get_send_comp()?;
        assert_eq!(wc.status, WcStatus::Success, "wc: {:?}", wc);
        println!("get recv comp");
        let wc = comm.recv_id.get_recv_comp()?;
        assert_eq!(wc.status, WcStatus::Success, "wc: {:?}", wc);

        println!("push new data");
        unsafe {
            comm.recv_id.post_recv(&mut comm.recv_mrs[i], .., i as u64)?;
        }
        println!("recv mr: {:?}",comm.recv_mrs[i].as_slice());
        // debug_assert_eq!(
        //     comm.recv_mrs[i].as_slice(),
        //     &vec![(recv_rank + 1) as u8; opts.msg_size],
        // );
        }

    Ok(())
}


fn main() {
    let opts = Opts::from_args();
    let comm = Communicator::new(&opts).expect("Create communicator failed");
    run_single_thread(&opts,comm);
}
