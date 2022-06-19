//! All-to-all benchmark. The purpose of this program is to test if koala works under different
//! threading models.
use std::io::Read;
use std::net::{SocketAddr, ToSocketAddrs};
use std::path::PathBuf;
use std::thread;
use std::time;

use clap::arg_enum;
use structopt::StructOpt;

use libkoala::cm;
use libkoala::verbs::{MemoryRegion, SendFlags, WcStatus};

arg_enum! {
    #[derive(Debug, Clone, Copy)]
    pub enum Mode {
        // single-thread
        St,
        // multiple-thread
        Mt,
        // user-level thread
        Ult,
    }
}

const DEFAULT_PORT: &str = "5000";

#[derive(StructOpt, Debug, Clone)]
#[structopt(about = "All-to-all benchmark.")]
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

    /// Total number of iterations.
    #[structopt(short, long, default_value = "1000")]
    pub total_iters: usize,

    /// Number of warmup iterations.
    #[structopt(short, long, default_value = "100")]
    pub warm_iters: usize,

    /// Message size.
    #[structopt(short, long, default_value = "65536")]
    pub msg_size: usize,

    /// The mode to use.
    #[structopt(possible_values = &Mode::variants(), case_insensitive = true)]
    pub mode: Mode,
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
        let mut file = std::fs::File::open(opts.hostfile.as_ref().unwrap()).expect("open");
        let mut content = String::new();
        file.read_to_string(&mut content).unwrap();
        content
            .split('\n')
            .map(|x| x.trim())
            .filter(|x| !x.is_empty() && !x.starts_with('#'))
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
                .expect("resolve address failed")
        })
        .collect()
}

struct Communicator {
    ids: Vec<cm::CmId>,
    recv_mrs: Vec<MemoryRegion<u8>>,
    send_mrs: Vec<MemoryRegion<u8>>,
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

        let num_peers = hosts.len() - 1;

        // create a listener
        let listener = cm::CmIdListener::bind(listen_addr)?;

        let mut ids = Vec::with_capacity(num_peers);
        let mut recv_mrs = Vec::with_capacity(num_peers);
        let mut send_mrs = Vec::with_capacity(num_peers);

        // establish connections.
        // a worker only accepts from others with higher ranks
        for _addr in &hosts[rank + 1..] {
            let mut builder = listener.get_request()?;
            let pre_id = builder.set_max_send_wr(2).set_max_recv_wr(2).build()?;

            // allocate messages before accepting
            let mut recv_mr: MemoryRegion<u8> = pre_id.alloc_msgs(opts.msg_size)?;
            for _ in 0..2 {
                unsafe {
                    pre_id.post_recv(&mut recv_mr, .., 0)?;
                }
            }

            let id = pre_id.accept(None)?;
            ids.push(id);
            recv_mrs.push(recv_mr);
        }

        // and only connects to those with lower ranks
        for addr in &hosts[..rank] {
            // connect retry
            let mut retry = 0;
            let (id, recv_mr) = loop {
                let builder = cm::CmIdBuilder::new()
                    .set_max_recv_wr(2)
                    .set_max_recv_wr(2)
                    .resolve_route(addr)?;

                let pre_id = builder.build()?;

                // allocate messages before connecting
                let mut recv_mr: MemoryRegion<u8> = pre_id.alloc_msgs(opts.msg_size)?;
                for _ in 0..2 {
                    unsafe {
                        pre_id.post_recv(&mut recv_mr, .., 0)?;
                    }
                }

                match pre_id.connect(None) {
                    Ok(id) => break (id, recv_mr),
                    Err(libkoala::Error::Connect(e)) => {
                        eprintln!("connect error: {}, connect retrying...", e);
                        thread::sleep(time::Duration::from_millis(100));
                        retry += 1;
                        if retry >= 50 {
                            return Err(e.into());
                        }
                    }
                    Err(e) => panic!("connect: {}", e),
                }
            };
            ids.push(id);
            recv_mrs.push(recv_mr);
        }

        // allocate send_mrs for each peer
        for id in &ids {
            let send_mr = {
                let mut mr = id.alloc_msgs(opts.msg_size)?;
                mr.fill(42u8);
                mr
            };
            send_mrs.push(send_mr);
        }

        Ok(Communicator {
            ids,
            recv_mrs,
            send_mrs,
        })
    }
}

fn run_single_thread(opts: &Opts, mut comm: Communicator) -> anyhow::Result<()> {
    let mut start = time::Instant::now();

    for iter in 0..opts.warm_iters + opts.total_iters {
        if iter == opts.warm_iters {
            start = time::Instant::now();
        }

        // post send all
        for (i, id) in comm.ids.iter().enumerate() {
            unsafe {
                id.post_send(&comm.send_mrs[i], .., i as u64, SendFlags::SIGNALED)?;
            }
        }

        // poll all completions
        for (i, id) in comm.ids.iter().enumerate() {
            let wc = id.get_send_comp()?;
            assert_eq!(wc.status, WcStatus::Success, "wc: {:?}", wc);
            let wc = id.get_recv_comp()?;
            assert_eq!(wc.status, WcStatus::Success, "wc: {:?}", wc);
            unsafe {
                id.post_recv(&mut comm.recv_mrs[i], .., i as u64)?;
            }
        }

        debug_assert_eq!(
            comm.recv_mrs[iter % comm.ids.len()].as_slice(),
            &vec![42u8; opts.msg_size],
        );
    }

    let dura = start.elapsed();
    let world_size = comm.ids.len() + 1;
    eprintln!(
        "alltoall, {} workers, duration: {:?}, bandwidth: {} Gb/s",
        world_size,
        dura,
        8e-9 * opts.total_iters as f64 * opts.msg_size as f64 * (world_size - 1) as f64
            / dura.as_secs_f64()
    );
    Ok(())
}

fn run_multi_thread(opts: &Opts, mut comm: Communicator) -> anyhow::Result<()> {
    let start = time::Instant::now();
    let world_size = comm.ids.len() + 1;

    let mut handles = Vec::with_capacity(comm.ids.len());
    for id in comm.ids {
        let send_mr = comm.send_mrs.remove(0);
        let mut recv_mr = comm.recv_mrs.remove(0);
        let opts = opts.clone();
        let h = thread::spawn(move || -> anyhow::Result<()> {
            for _ in 0..opts.warm_iters + opts.total_iters {
                // post send
                unsafe {
                    id.post_send(&send_mr, .., 0, SendFlags::SIGNALED)?;
                }
                // poll completion and post recv
                let wc = id.get_send_comp()?;
                assert_eq!(wc.status, WcStatus::Success, "wc: {:?}", wc);
                let wc = id.get_recv_comp()?;
                assert_eq!(wc.status, WcStatus::Success, "wc: {:?}", wc);
                unsafe {
                    id.post_recv(&mut recv_mr, .., 0)?;
                }
            }
            Ok(())
        });
        handles.push(h);
    }

    for h in handles {
        h.join().unwrap().unwrap();
    }

    let dura = start.elapsed();
    eprintln!(
        "alltoall, {} workers, duration: {:?}, bandwidth: {} Gb/s",
        world_size,
        dura,
        8e-9 * (opts.warm_iters + opts.total_iters) as f64
            * opts.msg_size as f64
            * (world_size - 1) as f64
            / dura.as_secs_f64()
    );
    Ok(())
}

fn run_user_level_thread(_opts: &Opts, _comm: Communicator) -> anyhow::Result<()> {
    unimplemented!()
}

fn main() {
    let opts = Opts::from_args();
    let comm = Communicator::new(&opts).expect("Create communicator failed");

    match opts.mode {
        Mode::St => run_single_thread(&opts, comm).unwrap(),
        Mode::Mt => run_multi_thread(&opts, comm).unwrap(),
        Mode::Ult => run_user_level_thread(&opts, comm).unwrap(),
    }
}
