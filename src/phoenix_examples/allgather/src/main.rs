use std::net::{SocketAddr, ToSocketAddrs};
use std::thread;
use std::time;

use structopt::StructOpt;

use libphoenix::cm;
use libphoenix::verbs::{MemoryRegion, WcStatus, SendFlags};


#[derive(StructOpt, Debug, Clone)]
#[structopt(about = "Allgather benchmark.")]
pub struct Opts {
    /// The addresses of the workers, seperated by ','. Each can be an IP address or domain name.
    #[structopt(short, long)]
    pub hosts: String,

    /// The port number to use if not given.
    #[structopt(short, long, default_value = "5000")]
    pub port: u16,

    /// Total number of iterations.
    #[structopt(short, long, default_value = "1000")]
    pub total_iters: usize,

    /// Number of warmup iterations.
    #[structopt(short, long, default_value = "100")]
    pub warm_iters: usize,

    /// Message size.
    #[structopt(short, long, default_value = "1024000")]
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
    send_mr: MemoryRegion<u8>,
    recv_id: cm::CmId,
    recv_mrs: Vec<MemoryRegion<u8>>,
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

        // connect in a ring setup
        let (send_id, recv_id, recv_mrs) = {
            // if it is the last host, then connect to left peer first and accept from right peer
            // if it is any other host, accept first, then connect after
            // this avoids a deadlock situation
            if rank == hosts.len() - 1 { 
                let (recv_id, recv_mrs) = Communicator::connect(left_peer_addr, opts.msg_size)?;
                let send_id = Communicator::accept(listen_addr)?;
                (send_id,recv_id,recv_mrs)
            } else{
                let send_id = Communicator::accept(listen_addr)?;
                let (recv_id, recv_mrs) = Communicator::connect(left_peer_addr, opts.msg_size)?;
                (send_id,recv_id,recv_mrs)
            }
        };
        let send_mr = {
            let mut mr = send_id.alloc_msgs(opts.msg_size)?;
            mr.fill(42u8);
            mr
        };

        Ok(Communicator {
            send_id,
            send_mr,
            recv_id,
            recv_mrs,
        })
    }

    fn accept (listen_addr:&SocketAddr) ->  Result<cm::CmId, libphoenix::Error>{
        // create a listener
        let listener = cm::CmIdListener::bind(listen_addr)?;

        // accept node with rank + 1
        let mut builder = listener.get_request()?;
        let pre_id = builder.set_max_send_wr(2).set_max_recv_wr(2).build()?;

        // socket accepts connections
        pre_id.accept(None)
    }

    fn connect(peer_addr:&SocketAddr, msg_size:usize)->Result<(cm::CmId,Vec<MemoryRegion<u8>>), libphoenix::Error> {
        // connect to node with rank - 1
        let mut retry = 0;
        let (recv_id,recv_mrs) = loop {
            let builder = cm::CmIdBuilder::new()
                .set_max_recv_wr(2)
                .set_max_recv_wr(2)
                .resolve_route(peer_addr)?;
            let pre_id = builder.build()?;
            
            // wrap in a vec to fix a borrowing compiler error
            let mut recv_mrs = Vec::with_capacity(1);

            // allocate message before connecting
            let mut recv_mr: MemoryRegion<u8> = pre_id.alloc_msgs(msg_size)?;
            for _ in 0..2 {
                unsafe {
                    pre_id.post_recv(&mut recv_mr, .., 0)?;
                }
            }
            recv_mrs.push(recv_mr);
            
            
            match pre_id.connect(None) {
                Ok(id) => {
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
    let mut start = time::Instant::now();
    let num_hosts = get_host_list(opts).len();

    for iter in 0..opts.warm_iters + opts.total_iters {
        if iter == opts.warm_iters {
            start = time::Instant::now();
        }

        // send/receive n-1 times to distribute all information to all nodes
        for i in 0..(num_hosts - 1){
            unsafe{
                comm.send_id.post_send(&comm.send_mr, .., i as u64, SendFlags::SIGNALED)?;
            }
            let wc = comm.send_id.get_send_comp()?;
            assert_eq!(wc.status, WcStatus::Success, "wc: {:?}", wc);
            let wc = comm.recv_id.get_recv_comp()?;
            assert_eq!(wc.status, WcStatus::Success, "wc: {:?}", wc);

            unsafe {
                comm.recv_id.post_recv(&mut comm.recv_mrs[0], .., i as u64)?;
            }

            debug_assert_eq!(
                comm.recv_mrs[0].as_slice(),
                &vec![42u8; opts.msg_size],
            );
        }
    }

    let dura = start.elapsed();
    println!(
        "allgather, {} workers, duration: {:?}, bandwidth: {} Gb/s",
        num_hosts,
        dura,
        8e-9 * opts.total_iters as f64
            * opts.msg_size as f64
            * (num_hosts - 1) as f64
            / dura.as_secs_f64(),
    );

    Ok(())
}

// cargo run --bin allgather -- --hosts rdma0.danyang-06,rdma0.danyang-05,rdma0.danyang-04
fn main() {
    let opts = Opts::from_args();
    let comm = Communicator::new(&opts).expect("Create communicator failed");
    run_single_thread(&opts,comm).unwrap()
}
